-- migrate:up
ALTER TABLE pipeline_stage_results DROP CONSTRAINT pipeline_stage_results_status_check;
ALTER TABLE pipeline_stage_results ADD CONSTRAINT pipeline_stage_results_status_check CHECK (status IN ('pending', 'running', 'completed', 'failed', 'skipped'));

ALTER TABLE pipeline_stages DROP CONSTRAINT pipeline_stages_stage_type_check;
ALTER TABLE pipeline_stages ADD CONSTRAINT pipeline_stages_stage_type_check CHECK (stage_type IN ('sql', 'http', 'gcs_check', 'openrouter', 'email', 'code'));

-- migrate:down
ALTER TABLE pipeline_stage_results DROP CONSTRAINT pipeline_stage_results_status_check;
ALTER TABLE pipeline_stage_results ADD CONSTRAINT pipeline_stage_results_status_check CHECK (status IN ('pending', 'running', 'completed', 'failed'));

ALTER TABLE pipeline_stages DROP CONSTRAINT pipeline_stages_stage_type_check;
ALTER TABLE pipeline_stages ADD CONSTRAINT pipeline_stages_stage_type_check CHECK (stage_type IN ('sql', 'http', 'gcs_check', 'openrouter', 'email'));
