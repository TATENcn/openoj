CREATE FUNCTION openoj_extract_required_capabilities(payload bytea)
RETURNS text[]
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
    document jsonb;
    capability_count integer;
    distinct_count integer;
    capabilities text[];
    capability text;
BEGIN
    document := convert_from(payload, 'UTF8')::jsonb;
    IF jsonb_typeof(document -> 'required_capabilities') <> 'array' THEN
        RAISE EXCEPTION 'required_capabilities must be an array';
    END IF;

    SELECT count(*), count(DISTINCT value), array_agg(value ORDER BY value)
    INTO capability_count, distinct_count, capabilities
    FROM jsonb_array_elements_text(document -> 'required_capabilities');

    IF capability_count NOT BETWEEN 1 AND 64 OR capability_count <> distinct_count THEN
        RAISE EXCEPTION 'required_capabilities must contain 1..64 unique values';
    END IF;

    FOREACH capability IN ARRAY capabilities LOOP
        IF capability !~ '^[a-z0-9]+([.-][a-z0-9]+)*$' OR octet_length(capability) > 64 THEN
            RAISE EXCEPTION 'required_capabilities contains an invalid capability';
        END IF;
    END LOOP;

    RETURN capabilities;
END;
$$;

CREATE FUNCTION openoj_valid_required_capabilities(capabilities text[])
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
    capability text;
    previous text := NULL;
BEGIN
    IF cardinality(capabilities) NOT BETWEEN 1 AND 64 THEN
        RETURN false;
    END IF;

    FOREACH capability IN ARRAY capabilities LOOP
        IF capability IS NULL
            OR capability !~ '^[a-z0-9]+([.-][a-z0-9]+)*$'
            OR octet_length(capability) > 64
            OR (previous IS NOT NULL AND capability <= previous) THEN
            RETURN false;
        END IF;
        previous := capability;
    END LOOP;
    RETURN true;
END;
$$;

ALTER TABLE evaluation_tasks ADD COLUMN required_capabilities text[];

UPDATE evaluation_tasks AS task
SET required_capabilities = openoj_extract_required_capabilities(attempt.request_payload)
FROM evaluation_attempts AS attempt
WHERE attempt.attempt_id = task.attempt_id;

ALTER TABLE evaluation_tasks
    ALTER COLUMN required_capabilities SET NOT NULL,
    ADD CONSTRAINT evaluation_tasks_required_capabilities_valid
    CHECK (openoj_valid_required_capabilities(required_capabilities));

ALTER TABLE evaluation_attempts
    ADD COLUMN claim_operation_id varchar(64),
    ADD CONSTRAINT evaluation_attempts_claim_operation_format CHECK (
        claim_operation_id IS NULL
        OR claim_operation_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]*$'
    );

CREATE UNIQUE INDEX evaluation_attempts_claim_operation_unique
    ON evaluation_attempts (node_id, claim_operation_id)
    WHERE claim_operation_id IS NOT NULL;

CREATE INDEX evaluation_tasks_ready_capabilities_order
    ON evaluation_tasks USING gin (required_capabilities)
    WHERE state = 'ready';

UPDATE openoj_schema_metadata SET schema_version = 2 WHERE singleton;
