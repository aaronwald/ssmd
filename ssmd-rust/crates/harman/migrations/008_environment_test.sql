-- Allow 'test' environment for test exchange instances
ALTER TABLE sessions DROP CONSTRAINT IF EXISTS sessions_environment_check;
ALTER TABLE sessions ADD CONSTRAINT sessions_environment_check CHECK (environment IN ('prod', 'demo', 'test'));

INSERT INTO schema_migrations (version) VALUES ('008_environment_test') ON CONFLICT DO NOTHING;
