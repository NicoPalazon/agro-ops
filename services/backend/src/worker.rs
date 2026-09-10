use std::{collections::BTreeMap, future::Future, sync::Arc, time::Duration};

use ::time::OffsetDateTime;
use async_trait::async_trait;
use sqlx::PgPool;
use tokio::{
    sync::{mpsc, oneshot},
    time::{self, MissedTickBehavior},
};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    jobs::{
        ClaimedJob, JobType, OwnershipTransition, SafeErrorSummary, claim, mark_failed,
        mark_succeeded, recover_stale,
    },
    service_heartbeats::{WORKER_SERVICE_NAME, record_service_heartbeat},
};

const WALKING_SKELETON_TEST_JOB_ID: &str = "walking-skeleton-test-job";

/// In-memory work item retained only for the established runtime smoke path.
pub enum WorkerJob {
    WalkingSkeletonTest { completed: oneshot::Sender<()> },
}

pub fn walking_skeleton_test_job() -> (WorkerJob, oneshot::Receiver<()>) {
    let (completed_sender, completed_receiver) = oneshot::channel();
    (
        WorkerJob::WalkingSkeletonTest {
            completed: completed_sender,
        },
        completed_receiver,
    )
}

#[derive(Clone, Copy, Debug)]
pub struct WorkerSettings {
    pub heartbeat_interval: Duration,
    pub poll_interval: Duration,
    pub claim_batch_size: u16,
    pub stale_threshold: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobExecutionResult {
    Succeeded,
    Retry {
        next_attempt_at: OffsetDateTime,
        error: SafeErrorSummary,
    },
}

#[async_trait]
pub trait JobHandler: Send + Sync {
    fn job_type(&self) -> &JobType;

    async fn execute(&self, job: &ClaimedJob) -> JobExecutionResult;
}

#[derive(Default)]
pub struct JobDispatcher {
    handlers: BTreeMap<String, Arc<dyn JobHandler>>,
}

impl JobDispatcher {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn register(&mut self, handler: Arc<dyn JobHandler>) -> Result<(), DuplicateJobType> {
        let job_type = handler.job_type().as_str().to_owned();
        if self.handlers.contains_key(&job_type) {
            return Err(DuplicateJobType(job_type));
        }
        self.handlers.insert(job_type, handler);
        Ok(())
    }

    pub fn supported_types(&self) -> Vec<JobType> {
        self.handlers
            .keys()
            .map(|job_type| {
                JobType::new(job_type.clone()).expect("registered job types were already validated")
            })
            .collect()
    }

    async fn execute(&self, job: &ClaimedJob) -> Option<JobExecutionResult> {
        Some(self.handlers.get(job.job_type.as_str())?.execute(job).await)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateJobType(String);

impl std::fmt::Display for DuplicateJobType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "duplicate job handler registration for {}",
            self.0
        )
    }
}

impl std::error::Error for DuplicateJobType {}

pub struct Worker {
    settings: WorkerSettings,
    db: Option<PgPool>,
    worker_id: Uuid,
    dispatcher: JobDispatcher,
    event_sender: Option<mpsc::UnboundedSender<WorkerEvent>>,
}

impl Worker {
    pub fn new(settings: WorkerSettings, db: PgPool, dispatcher: JobDispatcher) -> Self {
        Self {
            settings,
            db: Some(db),
            worker_id: Uuid::new_v4(),
            dispatcher,
            event_sender: None,
        }
    }

    pub fn worker_id(&self) -> Uuid {
        self.worker_id
    }

    pub async fn run<S>(self, mut in_memory_jobs: mpsc::Receiver<WorkerJob>, shutdown: S)
    where
        S: Future<Output = ()>,
    {
        let mut heartbeat = time::interval(self.settings.heartbeat_interval);
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
        heartbeat.tick().await;
        let mut queue_poll = time::interval(self.settings.poll_interval);
        queue_poll.set_missed_tick_behavior(MissedTickBehavior::Skip);

        tokio::pin!(shutdown);
        let mut in_memory_jobs_open = true;

        loop {
            tokio::select! {
                _ = heartbeat.tick() => self.heartbeat().await,
                _ = queue_poll.tick() => self.process_queue_once().await,
                job = in_memory_jobs.recv(), if in_memory_jobs_open => {
                    match job {
                        Some(job) => self.process_in_memory(job).await,
                        None => in_memory_jobs_open = false,
                    }
                }
                _ = &mut shutdown => {
                    info!(service = "worker", worker_id = %self.worker_id, "worker shutdown requested");
                    self.notify(WorkerEvent::Stopped);
                    break;
                }
            }
        }
    }

    pub async fn process_queue_once(&self) {
        let Some(db) = &self.db else {
            return;
        };

        match recover_stale(
            db,
            self.settings.stale_threshold,
            self.settings.claim_batch_size,
        )
        .await
        {
            Ok(recovered) if recovered.pending + recovered.exhausted > 0 => info!(
                service = "worker",
                worker_id = %self.worker_id,
                recovered_pending = recovered.pending,
                recovered_exhausted = recovered.exhausted,
                "stale jobs recovered"
            ),
            Ok(_) => {}
            Err(error) => {
                warn!(service = "worker", worker_id = %self.worker_id, %error, "failed to recover stale jobs");
                return;
            }
        }

        let supported_types = self.dispatcher.supported_types();
        let jobs = match claim(
            db,
            self.worker_id,
            &supported_types,
            self.settings.claim_batch_size,
        )
        .await
        {
            Ok(jobs) => jobs,
            Err(error) => {
                warn!(service = "worker", worker_id = %self.worker_id, %error, "failed to claim jobs");
                return;
            }
        };

        for job in jobs {
            self.execute_claimed(db, job).await;
        }
    }

    async fn execute_claimed(&self, db: &PgPool, job: ClaimedJob) {
        let Some(result) = self.dispatcher.execute(&job).await else {
            warn!(
                service = "worker",
                worker_id = %self.worker_id,
                job_id = %job.id,
                job_type = %job.job_type,
                "claimed job no longer has a registered handler; ownership left for stale recovery"
            );
            return;
        };

        let transition = match result {
            JobExecutionResult::Succeeded => mark_succeeded(db, job.id, self.worker_id).await,
            JobExecutionResult::Retry {
                next_attempt_at,
                error,
            } => mark_failed(db, job.id, self.worker_id, next_attempt_at, &error).await,
        };

        match transition {
            Ok(OwnershipTransition::Applied(updated)) => info!(
                service = "worker",
                worker_id = %self.worker_id,
                job_id = %updated.id,
                job_type = %updated.job_type,
                state = updated.state.as_str(),
                attempts = updated.attempts,
                "job execution persisted"
            ),
            Ok(OwnershipTransition::OwnershipLost) => warn!(
                service = "worker",
                worker_id = %self.worker_id,
                job_id = %job.id,
                job_type = %job.job_type,
                "job ownership was lost before execution result could be persisted"
            ),
            Err(error) => warn!(
                service = "worker",
                worker_id = %self.worker_id,
                job_id = %job.id,
                job_type = %job.job_type,
                %error,
                "failed to persist job execution result"
            ),
        }
    }

    async fn heartbeat(&self) {
        if let Some(db) = &self.db
            && let Err(error) = record_service_heartbeat(db, WORKER_SERVICE_NAME).await
        {
            warn!(service = "worker", %error, "failed to persist worker heartbeat");
        }
        info!(service = "worker", worker_id = %self.worker_id, "worker heartbeat");
        self.notify(WorkerEvent::Heartbeat);
    }

    async fn process_in_memory(&self, job: WorkerJob) {
        match job {
            WorkerJob::WalkingSkeletonTest { completed } => {
                info!(
                    service = "worker",
                    job_id = WALKING_SKELETON_TEST_JOB_ID,
                    "worker job received"
                );
                self.notify(WorkerEvent::JobReceived(WALKING_SKELETON_TEST_JOB_ID));
                tokio::task::yield_now().await;
                info!(
                    service = "worker",
                    job_id = WALKING_SKELETON_TEST_JOB_ID,
                    "worker job completed"
                );
                self.notify(WorkerEvent::JobCompleted(WALKING_SKELETON_TEST_JOB_ID));
                let _ = completed.send(());
            }
        }
    }

    fn notify(&self, event: WorkerEvent) {
        if let Some(event_sender) = &self.event_sender {
            let _ = event_sender.send(event);
        }
    }

    #[cfg(test)]
    fn with_event_sender(
        heartbeat_interval: Duration,
        event_sender: mpsc::UnboundedSender<WorkerEvent>,
    ) -> Self {
        Self {
            settings: WorkerSettings {
                heartbeat_interval,
                poll_interval: Duration::from_secs(60),
                claim_batch_size: 1,
                stale_threshold: Duration::from_secs(60),
            },
            db: None,
            worker_id: Uuid::new_v4(),
            dispatcher: JobDispatcher::empty(),
            event_sender: Some(event_sender),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum WorkerEvent {
    Heartbeat,
    JobReceived(&'static str),
    JobCompleted(&'static str),
    Stopped,
}

#[cfg(test)]
mod tests {
    use tokio::{sync::oneshot, time::timeout};

    use super::*;

    #[test]
    fn separate_worker_process_instances_have_distinct_execution_identities() {
        let (sender, _) = mpsc::unbounded_channel();
        let first = Worker::with_event_sender(Duration::from_secs(1), sender.clone());
        let second = Worker::with_event_sender(Duration::from_secs(1), sender);
        assert_ne!(first.worker_id(), second.worker_id());
    }

    #[tokio::test]
    async fn emits_heartbeat_and_stops_cleanly() {
        let (job_sender, job_receiver) = mpsc::channel(1);
        let (event_sender, mut events) = mpsc::unbounded_channel();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let worker = Worker::with_event_sender(Duration::from_millis(5), event_sender);
        let worker_task = tokio::spawn(worker.run(job_receiver, async {
            let _ = shutdown_receiver.await;
        }));

        assert_eq!(
            timeout(Duration::from_secs(1), events.recv())
                .await
                .expect("heartbeat must arrive before the timeout"),
            Some(WorkerEvent::Heartbeat)
        );
        shutdown_sender
            .send(())
            .expect("worker must still be running");
        assert_eq!(
            timeout(Duration::from_secs(1), events.recv())
                .await
                .expect("stop event must arrive before the timeout"),
            Some(WorkerEvent::Stopped)
        );
        worker_task.await.expect("worker task must finish cleanly");
        drop(job_sender);
    }

    #[tokio::test]
    async fn receives_and_completes_one_test_job_exactly_once() {
        let (job_sender, job_receiver) = mpsc::channel(1);
        let (event_sender, mut events) = mpsc::unbounded_channel();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let worker = Worker::with_event_sender(Duration::from_secs(60), event_sender);
        let (job, completed) = walking_skeleton_test_job();
        job_sender
            .send(job)
            .await
            .expect("worker queue must be open");
        let worker_task = tokio::spawn(worker.run(job_receiver, async {
            let _ = shutdown_receiver.await;
        }));

        timeout(Duration::from_secs(1), completed)
            .await
            .expect("test job must finish before the timeout")
            .expect("test job completion sender must remain available");
        shutdown_sender
            .send(())
            .expect("worker must still be running");
        worker_task.await.expect("worker task must finish cleanly");

        let observed: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
        assert_eq!(
            observed,
            vec![
                WorkerEvent::JobReceived(WALKING_SKELETON_TEST_JOB_ID),
                WorkerEvent::JobCompleted(WALKING_SKELETON_TEST_JOB_ID),
                WorkerEvent::Stopped,
            ]
        );
    }
}
