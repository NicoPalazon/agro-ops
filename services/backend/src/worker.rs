use std::{future::Future, time::Duration};

use sqlx::PgPool;
use tokio::{
    sync::{mpsc, oneshot},
    time::{self, MissedTickBehavior},
};
use tracing::{info, warn};

use crate::service_heartbeats::{WORKER_SERVICE_NAME, record_service_heartbeat};

const WALKING_SKELETON_TEST_JOB_ID: &str = "walking-skeleton-test-job";

/// In-memory work item used only to prove the Walking Skeleton worker flow.
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

pub struct Worker {
    heartbeat_interval: Duration,
    heartbeat_db: Option<PgPool>,
    event_sender: Option<mpsc::UnboundedSender<WorkerEvent>>,
}

impl Worker {
    pub fn new(heartbeat_interval: Duration, heartbeat_db: PgPool) -> Self {
        Self {
            heartbeat_interval,
            heartbeat_db: Some(heartbeat_db),
            event_sender: None,
        }
    }

    pub async fn run<S>(self, mut jobs: mpsc::Receiver<WorkerJob>, shutdown: S)
    where
        S: Future<Output = ()>,
    {
        let mut heartbeat = time::interval(self.heartbeat_interval);
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
        heartbeat.tick().await;

        tokio::pin!(shutdown);
        let mut jobs_open = true;

        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    self.heartbeat().await;
                }
                job = jobs.recv(), if jobs_open => {
                    match job {
                        Some(job) => self.process(job).await,
                        None => jobs_open = false,
                    }
                }
                _ = &mut shutdown => {
                    info!(service = "worker", "worker shutdown requested");
                    self.notify(WorkerEvent::Stopped);
                    break;
                }
            }
        }
    }

    async fn heartbeat(&self) {
        if let Some(db) = &self.heartbeat_db
            && let Err(error) = record_service_heartbeat(db, WORKER_SERVICE_NAME).await
        {
            warn!(service = "worker", %error, "failed to persist worker heartbeat");
        }

        info!(service = "worker", "worker heartbeat");
        self.notify(WorkerEvent::Heartbeat);
    }

    async fn process(&self, job: WorkerJob) {
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
            heartbeat_interval,
            heartbeat_db: None,
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
    use std::time::Duration;

    use tokio::{sync::oneshot, time::timeout};

    use super::*;

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
        assert_eq!(
            observed
                .iter()
                .filter(|event| matches!(event, WorkerEvent::JobCompleted(_)))
                .count(),
            1
        );
    }
}
