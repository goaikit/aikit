use aikit_sdk::{
    get_agent_status as get_agent_status_impl, get_installed_agents as get_installed_agents_impl,
    is_agent_available as is_agent_available_impl, is_runnable, run_agent as run_agent_impl,
    runnable_agents, AgentConfig, AgentStatus, DeployConcept, DeployError, RunOptions,
};
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::PathBuf;

// Removed PyDeployError struct and its #[pyclass]

// Helper to convert Result<T, DeployError> to PyResult<T>
// This replaces the impl From<DeployError> for PyErr, addressing the orphan rule
fn to_py_result<T>(result: Result<T, DeployError>) -> PyResult<T> {
    result.map_err(|e| PyException::new_err(format!("{}", e)))
}

// Implement the PyO3 bindings for DeployConcept enum.
#[pyclass(from_py_object)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PyDeployConcept {
    Command,
    Skill,
    Subagent,
    InstructionFile,
}

impl From<DeployConcept> for PyDeployConcept {
    fn from(concept: DeployConcept) -> Self {
        match concept {
            DeployConcept::Command => PyDeployConcept::Command,
            DeployConcept::Skill => PyDeployConcept::Skill,
            DeployConcept::Subagent => PyDeployConcept::Subagent,
            DeployConcept::InstructionFile => PyDeployConcept::InstructionFile,
        }
    }
}

impl From<PyDeployConcept> for DeployConcept {
    fn from(concept: PyDeployConcept) -> Self {
        match concept {
            PyDeployConcept::Command => DeployConcept::Command,
            PyDeployConcept::Skill => DeployConcept::Skill,
            PyDeployConcept::Subagent => DeployConcept::Subagent,
            PyDeployConcept::InstructionFile => DeployConcept::InstructionFile,
        }
    }
}

#[pyclass(from_py_object)]
#[derive(Debug, Clone)]
pub struct PyAgentConfig {
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub commands_dir: String,
    #[pyo3(get)]
    pub skills_dir: Option<String>,
    #[pyo3(get)]
    pub agents_dir: Option<String>,
    #[pyo3(get)]
    pub scripts_dir: Option<String>,
    #[pyo3(get)]
    pub instruction_file: Option<String>,
}

#[pyclass(from_py_object)]
#[derive(Debug, Clone)]
pub struct PyRunOptions {
    #[pyo3(get, set)]
    pub model: Option<String>,
    #[pyo3(get, set)]
    pub yolo: bool,
    #[pyo3(get, set)]
    pub stream: bool,
}

#[pyclass(from_py_object)]
#[derive(Debug, Clone)]
pub struct PyAgentStatus {
    #[pyo3(get)]
    pub available: bool,
    #[pyo3(get)]
    pub reason: Option<String>,
}

impl From<AgentConfig> for PyAgentConfig {
    fn from(config: AgentConfig) -> Self {
        PyAgentConfig {
            name: config.name,
            commands_dir: config.commands_dir,
            skills_dir: config.skills_dir,
            agents_dir: config.agents_dir,
            scripts_dir: config.scripts_dir,
            instruction_file: config.instruction_file,
        }
    }
}

impl From<AgentStatus> for PyAgentStatus {
    fn from(status: AgentStatus) -> Self {
        PyAgentStatus {
            available: status.available,
            reason: status.reason.map(|r| r.to_string()),
        }
    }
}

#[pymethods]
impl PyAgentConfig {
    #[pyo3(name = "key")]
    fn py_key(&self) -> String {
        aikit_sdk::AgentConfig {
            name: self.name.clone(),
            commands_dir: self.commands_dir.clone(),
            skills_dir: self.skills_dir.clone(),
            agents_dir: self.agents_dir.clone(),
            scripts_dir: self.scripts_dir.clone(),
            instruction_file: self.instruction_file.clone(),
        }
        .key()
    }
}

#[pyfunction]
fn subagent_filename(agent_key: &str, name: &str) -> String {
    aikit_sdk::subagent_filename(agent_key, name)
}

#[pyfunction]
fn command_filename(agent_key: &str, name: &str) -> String {
    aikit_sdk::command_filename(agent_key, name)
}

#[pyfunction]
fn subagent_path(project_root: PathBuf, agent_key: &str, name: &str) -> PyResult<String> {
    to_py_result(
        aikit_sdk::subagent_path(&project_root, agent_key, name)
            .map(|path| path.to_string_lossy().into_owned()),
    )
}

#[pyfunction]
fn commands_dir(project_root: PathBuf, agent_key: &str) -> PyResult<String> {
    to_py_result(
        aikit_sdk::commands_dir(&project_root, agent_key)
            .map(|path| path.to_string_lossy().into_owned()),
    )
}

#[pyfunction]
fn skill_dir(project_root: PathBuf, agent_key: &str, skill_name: &str) -> PyResult<String> {
    to_py_result(
        aikit_sdk::skill_dir(&project_root, agent_key, skill_name)
            .map(|path| path.to_string_lossy().into_owned()),
    )
}

#[pyfunction]
fn validate_agent_key(key: &str) -> PyResult<()> {
    to_py_result(aikit_sdk::validate_agent_key(key))
}

#[pyfunction]
fn all_agents() -> Vec<PyAgentConfig> {
    aikit_sdk::all_agents()
        .into_iter()
        .map(PyAgentConfig::from)
        .collect()
}

#[pyfunction]
fn agent(key: &str) -> Option<PyAgentConfig> {
    aikit_sdk::agent(key).map(PyAgentConfig::from)
}

#[pyfunction]
fn deploy_command(
    agent_key: &str,
    project_root: PathBuf,
    name: &str,
    content: &str,
) -> PyResult<String> {
    to_py_result(
        aikit_sdk::deploy_command(agent_key, &project_root, name, content)
            .map(|path| path.to_string_lossy().into_owned()),
    )
}

#[pyfunction]
fn deploy_skill(
    agent_key: &str,
    project_root: PathBuf,
    skill_name: &str,
    skill_md_content: &str,
    optional_scripts: Option<Vec<(String, Vec<u8>)>>,
) -> PyResult<String> {
    let scripts_data: Option<Vec<(&'static str, &'static [u8])>> =
        optional_scripts.map(|scripts| {
            scripts
                .into_iter()
                .map(|(name, content)| {
                    (name.leak() as &'static str, content.leak() as &'static [u8])
                })
                .collect()
        });

    to_py_result(
        aikit_sdk::deploy_skill(
            agent_key,
            &project_root,
            skill_name,
            skill_md_content,
            scripts_data.as_deref(),
        )
        .map(|path| path.to_string_lossy().into_owned()),
    )
}

#[pyfunction]
fn deploy_subagent(
    agent_key: &str,
    project_root: PathBuf,
    name: &str,
    content: &str,
) -> PyResult<String> {
    to_py_result(
        aikit_sdk::deploy_subagent(agent_key, &project_root, name, content)
            .map(|path| path.to_string_lossy().into_owned()),
    )
}

impl From<RunOptions> for PyRunOptions {
    fn from(options: RunOptions) -> Self {
        PyRunOptions {
            model: options.model,
            yolo: options.yolo,
            stream: options.stream,
        }
    }
}

impl From<PyRunOptions> for RunOptions {
    fn from(options: PyRunOptions) -> Self {
        RunOptions {
            model: options.model,
            yolo: options.yolo,
            stream: options.stream,
        }
    }
}

#[pymethods]
impl PyRunOptions {
    #[new]
    #[pyo3(signature = (model=None, yolo=false, stream=false))]
    fn new(model: Option<String>, yolo: bool, stream: bool) -> Self {
        PyRunOptions {
            model,
            yolo,
            stream,
        }
    }
}

#[pyfunction]
fn run_agent(
    py: Python<'_>,
    agent_key: &str,
    prompt: &str,
    model: Option<String>,
    yolo: bool,
    stream: bool,
) -> PyResult<Py<PyDict>> {
    let options = RunOptions {
        model,
        yolo,
        stream,
    };

    let result = run_agent_impl(agent_key, prompt, options)
        .map_err(|e| PyException::new_err(format!("{}", e)))?;

    let dict = PyDict::new(py);
    dict.set_item("status_code", result.status.code())?;
    dict.set_item("stdout", result.stdout)?;
    dict.set_item("stderr", result.stderr)?;
    Ok(dict.into())
}

#[pyfunction]
fn runnable_agents_list() -> Vec<String> {
    runnable_agents().iter().map(|s| s.to_string()).collect()
}

#[pyfunction]
fn is_runnable_py(agent_key: &str) -> bool {
    is_runnable(agent_key)
}

#[pyfunction]
fn is_agent_available(agent_key: &str) -> bool {
    is_agent_available_impl(agent_key)
}

#[pyfunction]
fn is_agent_available_py(agent_key: &str) -> bool {
    is_agent_available_impl(agent_key)
}

#[pyfunction]
fn get_installed_agents() -> Vec<String> {
    get_installed_agents_impl()
}

#[pyfunction]
fn get_installed_agents_py() -> Vec<String> {
    get_installed_agents_impl()
}

#[pyfunction]
fn get_agent_status(py: Python<'_>) -> PyResult<Py<PyDict>> {
    let status_map = get_agent_status_impl();
    let dict = PyDict::new(py);

    for (agent_key, agent_status) in status_map {
        let status_dict = PyDict::new(py);
        status_dict.set_item("available", agent_status.available)?;

        if let Some(reason) = agent_status.reason {
            status_dict.set_item("reason", reason.to_string())?;
        } else {
            status_dict.set_item("reason", py.None())?;
        }

        dict.set_item(agent_key, status_dict)?;
    }

    Ok(dict.into())
}

#[pyfunction]
fn get_agent_status_py(py: Python<'_>) -> PyResult<Py<PyDict>> {
    get_agent_status(py)
}

#[pymodule]
fn aikit_py(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("DeployError", _py.get_type::<PyException>())?;

    m.add_class::<PyDeployConcept>()?;
    m.add_class::<PyAgentConfig>()?;
    m.add_class::<PyRunOptions>()?;
    m.add_class::<PyAgentStatus>()?;
    m.add_wrapped(wrap_pyfunction!(subagent_filename))?;
    m.add_wrapped(wrap_pyfunction!(command_filename))?;
    m.add_wrapped(wrap_pyfunction!(subagent_path))?;
    m.add_wrapped(wrap_pyfunction!(commands_dir))?;
    m.add_wrapped(wrap_pyfunction!(skill_dir))?;
    m.add_wrapped(wrap_pyfunction!(validate_agent_key))?;
    m.add_wrapped(wrap_pyfunction!(all_agents))?;
    m.add_wrapped(wrap_pyfunction!(agent))?;
    m.add_wrapped(wrap_pyfunction!(deploy_command))?;
    m.add_wrapped(wrap_pyfunction!(deploy_skill))?;
    m.add_wrapped(wrap_pyfunction!(deploy_subagent))?;
    m.add_wrapped(wrap_pyfunction!(run_agent))?;
    m.add_wrapped(wrap_pyfunction!(runnable_agents_list))?;
    m.add_wrapped(wrap_pyfunction!(is_runnable_py))?;
    m.add_wrapped(wrap_pyfunction!(is_agent_available))?;
    m.add_wrapped(wrap_pyfunction!(is_agent_available_py))?;
    m.add_wrapped(wrap_pyfunction!(get_installed_agents))?;
    m.add_wrapped(wrap_pyfunction!(get_installed_agents_py))?;
    m.add_wrapped(wrap_pyfunction!(get_agent_status))?;
    m.add_wrapped(wrap_pyfunction!(get_agent_status_py))?;
    Ok(())
}
