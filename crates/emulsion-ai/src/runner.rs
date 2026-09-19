//! ONNX Runtime sessions: loading, execution-provider choice and a cache so
//! a model is read from disk once per process.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ndarray::{ArrayD, IxDyn};
use ort::session::Session;
use ort::value::Tensor;

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("model not installed: {0}")]
    NotInstalled(String),
    #[error("ONNX Runtime: {0}")]
    Ort(String),
    #[error("model {0} has unexpected inputs or outputs: {1}")]
    Shape(String, String),
    #[error("{0}")]
    Cancelled(#[from] crate::jobs::Cancelled),
    #[error("{0}")]
    Other(String),
}

impl<T> From<ort::Error<T>> for RunError {
    fn from(e: ort::Error<T>) -> Self {
        RunError::Ort(e.to_string())
    }
}

/// Which execution provider sessions use; chosen once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Cpu,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Provider::Cpu => "CPU",
        }
    }
}

/// The provider this build runs on. Only the CPU provider is compiled in for
/// now: it works everywhere and the models here are sized for it. GPU
/// providers are a build feature to add when there is hardware to test on.
pub fn provider() -> Provider {
    Provider::Cpu
}

/// One loaded model with its input and output names.
pub struct Model {
    pub path: PathBuf,
    session: Mutex<Session>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

impl Model {
    pub fn load(path: &Path) -> Result<Arc<Model>, RunError> {
        if !path.exists() {
            return Err(RunError::NotInstalled(path.display().to_string()));
        }
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, 16);
        let session = Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
            .with_intra_threads(threads)?
            .commit_from_file(path)?;
        let inputs = session
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        let outputs = session
            .outputs()
            .iter()
            .map(|o| o.name().to_string())
            .collect();
        Ok(Arc::new(Model {
            path: path.to_path_buf(),
            session: Mutex::new(session),
            inputs,
            outputs,
        }))
    }

    /// Run with named f32 inputs; outputs come back by name as f32 arrays.
    pub fn run(
        &self,
        inputs: &[(&str, ArrayD<f32>)],
    ) -> Result<HashMap<String, ArrayD<f32>>, RunError> {
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let mut values: Vec<(String, ort::session::SessionInputValue)> =
            Vec::with_capacity(inputs.len());
        for (name, arr) in inputs {
            let shape: Vec<i64> = arr.shape().iter().map(|&d| d as i64).collect();
            let data: Vec<f32> = arr.iter().copied().collect();
            let t = Tensor::<f32>::from_array((shape, data))?;
            values.push((name.to_string(), t.into()));
        }
        let outputs = session.run(values)?;
        let mut out = HashMap::new();
        for name in &self.outputs {
            if let Some(v) = outputs.get(name.as_str()) {
                let (shape, data) = v.try_extract_tensor::<f32>()?;
                let dims: Vec<usize> = shape.iter().map(|&d| d.max(0) as usize).collect();
                let arr = ArrayD::from_shape_vec(IxDyn(&dims), data.to_vec())
                    .map_err(|e| RunError::Other(e.to_string()))?;
                out.insert(name.clone(), arr);
            }
        }
        Ok(out)
    }

    /// Run with mixed inputs (f32 and i64), for prompt encoders.
    pub fn run_mixed(
        &self,
        f32s: &[(&str, ArrayD<f32>)],
        i64s: &[(&str, ArrayD<i64>)],
    ) -> Result<HashMap<String, ArrayD<f32>>, RunError> {
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let mut values: Vec<(String, ort::session::SessionInputValue)> = Vec::new();
        for (name, arr) in f32s {
            let shape: Vec<i64> = arr.shape().iter().map(|&d| d as i64).collect();
            let t = Tensor::<f32>::from_array((shape, arr.iter().copied().collect::<Vec<_>>()))?;
            values.push((name.to_string(), t.into()));
        }
        for (name, arr) in i64s {
            let shape: Vec<i64> = arr.shape().iter().map(|&d| d as i64).collect();
            let t = Tensor::<i64>::from_array((shape, arr.iter().copied().collect::<Vec<_>>()))?;
            values.push((name.to_string(), t.into()));
        }
        let outputs = session.run(values)?;
        let mut out = HashMap::new();
        for name in &self.outputs {
            if let Some(v) = outputs.get(name.as_str()) {
                let (shape, data) = v.try_extract_tensor::<f32>()?;
                let dims: Vec<usize> = shape.iter().map(|&d| d.max(0) as usize).collect();
                let arr = ArrayD::from_shape_vec(IxDyn(&dims), data.to_vec())
                    .map_err(|e| RunError::Other(e.to_string()))?;
                out.insert(name.clone(), arr);
            }
        }
        Ok(out)
    }
}

/// Loaded models by path.
fn cache() -> &'static Mutex<HashMap<PathBuf, Arc<Model>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Model>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Load `path` once; later calls share the session.
pub fn model(path: &Path) -> Result<Arc<Model>, RunError> {
    if let Some(m) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(path) {
        return Ok(m.clone());
    }
    let m = Model::load(path)?;
    cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.to_path_buf(), m.clone());
    Ok(m)
}

/// Drop every cached session (after removing a model, or to free memory).
pub fn clear_cache() {
    cache().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Load the file `name` of model `id`, or say it is not installed.
pub fn model_file(id: &str, name: &str) -> Result<Arc<Model>, RunError> {
    let spec = crate::models::spec(id).ok_or_else(|| RunError::NotInstalled(id.into()))?;
    let f = spec
        .files
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| RunError::NotInstalled(format!("{id}/{name}")))?;
    if crate::models::status(spec) != crate::models::Status::Installed {
        return Err(RunError::NotInstalled(spec.name.into()));
    }
    model(&crate::models::file_path(spec, f))
}
