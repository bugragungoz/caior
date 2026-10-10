use std::path::PathBuf;

pub struct ExecutionContext {
    pub id: String,
    pub log_path: PathBuf,
    pub state_path: PathBuf,
    pub exit_code_path: PathBuf,
    pub script_path: PathBuf,
}

impl ExecutionContext {
    pub fn new() -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let temp = std::env::temp_dir();
        ExecutionContext {
            log_path: temp.join(format!("croxz_{id}.log")),
            state_path: temp.join(format!("croxz_{id}.state")),
            exit_code_path: temp.join(format!("croxz_{id}.exit")),
            script_path: temp.join(format!("croxz_{id}.ps1")),
            id,
        }
    }

    pub fn cleanup(&self) {
        for path in [&self.log_path, &self.state_path, &self.exit_code_path, &self.script_path] {
            let _ = std::fs::remove_file(path);
        }
    }
}
