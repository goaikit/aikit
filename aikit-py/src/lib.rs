use aikit_sdk::DeployConcept;
use aikit_sdk::{AgentConfig, DeployError};
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use std::path::PathBuf;

// Removed PyDeployError struct and its #[pyclass]

// Helper to convert Result<T, DeployError> to PyResult<T>
// This replaces the impl From<DeployError> for PyErr, addressing the orphan rule
fn to_py_result<T>(result: Result<T, DeployError>) -> PyResult<T> {
    result.map_err(|e| PyException::new_err(format!("{}", e)))
}

// Implement the PyO3 bindings for DeployConcept enum.
#[pyclass]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PyDeployConcept {
    Command,
    Skill,
    Subagent,
}

impl From<DeployConcept> for PyDeployConcept {
    fn from(concept: DeployConcept) -> Self {
        match concept {
            DeployConcept::Command => PyDeployConcept::Command,
            DeployConcept::Skill => PyDeployConcept::Skill,
            DeployConcept::Subagent => PyDeployConcept::Subagent,
        }
    }
}

impl From<PyDeployConcept> for DeployConcept {
    fn from(concept: PyDeployConcept) -> Self {
        match concept {
            PyDeployConcept::Command => DeployConcept::Command,
            PyDeployConcept::Skill => DeployConcept::Skill,
            PyDeployConcept::Subagent => DeployConcept::Subagent,
        }
    }
}

#[pyclass]
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
}

impl From<AgentConfig> for PyAgentConfig {
    fn from(config: AgentConfig) -> Self {
        PyAgentConfig {
            name: config.name,
            commands_dir: config.commands_dir,
            skills_dir: config.skills_dir,
            agents_dir: config.agents_dir,
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

#[pymodule]
fn aikit_py(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("DeployError", _py.get_type::<PyException>())?; // Corrected to get_type

    m.add_class::<PyDeployConcept>()?;
    m.add_class::<PyAgentConfig>()?;
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
    Ok(())
}
