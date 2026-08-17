CREATE TABLE openoj_schema_metadata (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    schema_version integer NOT NULL CHECK (schema_version >= 1)
);

INSERT INTO openoj_schema_metadata (singleton, schema_version) VALUES (true, 1);

CREATE TABLE artifacts (
    artifact_id varchar(64) PRIMARY KEY,
    digest varchar(71) NOT NULL,
    media_type varchar(129) NOT NULL,
    size_bytes bigint NOT NULL,
    sensitivity varchar(16) NOT NULL,
    CONSTRAINT artifacts_id_format CHECK (
        artifact_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
    ),
    CONSTRAINT artifacts_digest_format CHECK (digest ~ '^sha256:[0-9a-f]{64}$'),
    CONSTRAINT artifacts_media_type_bound CHECK (octet_length(media_type) BETWEEN 1 AND 129),
    CONSTRAINT artifacts_size_bound CHECK (size_bytes BETWEEN 0 AND 1099511627776),
    CONSTRAINT artifacts_sensitivity CHECK (sensitivity IN ('public', 'private', 'hidden'))
);

CREATE TABLE problem_versions (
    problem_version_id varchar(64) PRIMARY KEY,
    problem_id varchar(64) NOT NULL,
    digest varchar(71) NOT NULL,
    CONSTRAINT problem_versions_id_format CHECK (
        problem_version_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
        AND problem_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
    ),
    CONSTRAINT problem_versions_digest_format CHECK (digest ~ '^sha256:[0-9a-f]{64}$')
);

CREATE TABLE submissions (
    submission_id varchar(64) PRIMARY KEY,
    source_artifact_id varchar(64) NOT NULL REFERENCES artifacts (artifact_id),
    CONSTRAINT submissions_id_format CHECK (
        submission_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
    )
);

CREATE TABLE runtimes (
    runtime_id varchar(64) PRIMARY KEY,
    digest varchar(71) NOT NULL,
    CONSTRAINT runtimes_id_format CHECK (
        runtime_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
    ),
    CONSTRAINT runtimes_digest_format CHECK (digest ~ '^sha256:[0-9a-f]{64}$')
);

CREATE TABLE evaluations (
    evaluation_id varchar(64) PRIMARY KEY,
    request_id varchar(64) NOT NULL,
    creation_idempotency_key varchar(128) NOT NULL UNIQUE,
    problem_version_id varchar(64) NOT NULL REFERENCES problem_versions (problem_version_id),
    submission_id varchar(64) NOT NULL REFERENCES submissions (submission_id),
    runtime_id varchar(64) NOT NULL REFERENCES runtimes (runtime_id),
    initial_request bytea NOT NULL,
    state varchar(16) NOT NULL,
    current_attempt_id varchar(64) NOT NULL,
    terminal_result bytea,
    result_idempotency_key varchar(128) UNIQUE,
    created_at_ms bigint NOT NULL,
    updated_at_ms bigint NOT NULL,
    CONSTRAINT evaluations_id_format CHECK (
        evaluation_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
        AND request_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
        AND current_attempt_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
        AND creation_idempotency_key ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
        AND (
            result_idempotency_key IS NULL
            OR result_idempotency_key ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
        )
    ),
    CONSTRAINT evaluations_request_bound CHECK (octet_length(initial_request) BETWEEN 1 AND 262144),
    CONSTRAINT evaluations_state CHECK (state IN ('queued', 'leased', 'completed', 'failed', 'cancelled')),
    CONSTRAINT evaluations_result_pair CHECK (
        (terminal_result IS NULL) = (result_idempotency_key IS NULL)
        AND (terminal_result IS NULL OR octet_length(terminal_result) BETWEEN 1 AND 1048576)
    ),
    CONSTRAINT evaluations_time_bound CHECK (
        created_at_ms BETWEEN 0 AND 253402300799999
        AND updated_at_ms BETWEEN created_at_ms AND 253402300799999
    )
);

CREATE TABLE evaluation_attempts (
    attempt_id varchar(64) PRIMARY KEY,
    evaluation_id varchar(64) NOT NULL REFERENCES evaluations (evaluation_id),
    request_id varchar(64) NOT NULL,
    attempt_number bigint NOT NULL,
    attempt_idempotency_key varchar(128) NOT NULL UNIQUE,
    request_payload bytea NOT NULL,
    state varchar(16) NOT NULL,
    node_id varchar(64),
    lease_token varchar(64),
    lease_expires_at_ms bigint,
    created_at_ms bigint NOT NULL,
    updated_at_ms bigint NOT NULL,
    UNIQUE (evaluation_id, attempt_number),
    CONSTRAINT evaluation_attempts_id_format CHECK (
        attempt_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
        AND request_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
        AND attempt_idempotency_key ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
        AND (node_id IS NULL OR node_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$')
        AND (lease_token IS NULL OR lease_token ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$')
    ),
    CONSTRAINT evaluation_attempts_number_bound CHECK (attempt_number BETWEEN 1 AND 4294967295),
    CONSTRAINT evaluation_attempts_request_bound CHECK (
        octet_length(request_payload) BETWEEN 1 AND 262144
    ),
    CONSTRAINT evaluation_attempts_state CHECK (
        state IN ('queued', 'leased', 'completed', 'failed', 'cancelled', 'expired')
    ),
    CONSTRAINT evaluation_attempts_lease_fields CHECK (
        (node_id IS NULL) = (lease_token IS NULL)
        AND (lease_token IS NULL) = (lease_expires_at_ms IS NULL)
        AND (
            lease_expires_at_ms IS NULL
            OR lease_expires_at_ms BETWEEN 0 AND 253402300799999
        )
    ),
    CONSTRAINT evaluation_attempts_time_bound CHECK (
        created_at_ms BETWEEN 0 AND 253402300799999
        AND updated_at_ms BETWEEN created_at_ms AND 253402300799999
    )
);

ALTER TABLE evaluations
    ADD CONSTRAINT evaluations_current_attempt_fk
    FOREIGN KEY (current_attempt_id)
    REFERENCES evaluation_attempts (attempt_id)
    DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE evaluation_tasks (
    attempt_id varchar(64) PRIMARY KEY REFERENCES evaluation_attempts (attempt_id),
    state varchar(16) NOT NULL,
    created_at_ms bigint NOT NULL,
    updated_at_ms bigint NOT NULL,
    CONSTRAINT evaluation_tasks_state CHECK (
        state IN ('ready', 'leased', 'completed', 'cancelled', 'expired')
    ),
    CONSTRAINT evaluation_tasks_time_bound CHECK (
        created_at_ms BETWEEN 0 AND 253402300799999
        AND updated_at_ms BETWEEN created_at_ms AND 253402300799999
    )
);

CREATE INDEX evaluation_tasks_ready_order
    ON evaluation_tasks (created_at_ms, attempt_id)
    WHERE state = 'ready';
