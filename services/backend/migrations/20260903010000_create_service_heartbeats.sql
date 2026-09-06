CREATE TABLE service_heartbeats (
    service_name TEXT PRIMARY KEY,
    last_seen_at TIMESTAMPTZ NOT NULL
);
