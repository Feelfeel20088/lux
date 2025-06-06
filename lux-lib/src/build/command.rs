use shell_words::split;
use std::{
    collections::HashMap,
    io,
    path::Path,
    process::{ExitStatus, Stdio},
};
use thiserror::Error;
use tokio::process::Command;

use crate::{
    config::Config,
    lua_installation::LuaInstallation,
    lua_rockspec::{Build, BuildInfo, CommandBuildSpec},
    progress::{Progress, ProgressBar},
    tree::{RockLayout, Tree},
    variables::VariableSubstitutionError,
};

use super::external_dependency::ExternalDependencyInfo;
use super::utils;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("'build_command' and 'install_command' cannot be empty.")]
    EmptyCommand,
    #[error("error parsing command:\n{command}\n\nerror: {err}")]
    ParseError {
        err: shell_words::ParseError,
        command: String,
    },
    #[error("error executing command:\n{command}\n\nerror: {err}")]
    Io { err: io::Error, command: String },
    #[error("failed to execute command:\n{command}\n\nstatus: {status}\nstdout: {stdout}\nstderr: {stderr}")]
    CommandFailure {
        command: String,
        status: ExitStatus,
        stdout: String,
        stderr: String,
    },
    #[error(transparent)]
    VariableSubstitutionError(#[from] VariableSubstitutionError),
}

impl Build for CommandBuildSpec {
    type Err = CommandError;

    async fn run(
        self,
        output_paths: &RockLayout,
        no_install: bool,
        lua: &LuaInstallation,
        external_dependencies: &HashMap<String, ExternalDependencyInfo>,
        config: &Config,
        _tree: &Tree,
        build_dir: &Path,
        progress: &Progress<ProgressBar>,
    ) -> Result<BuildInfo, Self::Err> {
        progress.map(|bar| bar.set_message("Running build_command..."));
        run_command(
            &self.build_command,
            output_paths,
            lua,
            external_dependencies,
            config,
            build_dir,
        )
        .await?;
        if !no_install {
            progress.map(|bar| bar.set_message("Running install_command..."));
            run_command(
                &self.install_command,
                output_paths,
                lua,
                external_dependencies,
                config,
                build_dir,
            )
            .await?;
        }
        Ok(BuildInfo::default())
    }
}

async fn run_command(
    command: &str,
    output_paths: &RockLayout,
    lua: &LuaInstallation,
    external_dependencies: &HashMap<String, ExternalDependencyInfo>,
    config: &Config,
    build_dir: &Path,
) -> Result<(), CommandError> {
    let substituted_cmd =
        utils::substitute_variables(command, output_paths, lua, external_dependencies, config)?;
    let cmd_parts = split(&substituted_cmd).map_err(|err| CommandError::ParseError {
        err,
        command: substituted_cmd.clone(),
    })?;
    let (program, args) = cmd_parts.split_first().ok_or(CommandError::EmptyCommand)?;
    match Command::new(program)
        .args(args)
        .current_dir(build_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Err(err) => {
            return Err(CommandError::Io {
                err,
                command: substituted_cmd,
            })
        }
        Ok(child) => match child.wait_with_output().await {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                return Err(CommandError::CommandFailure {
                    command: substituted_cmd,
                    status: output.status,
                    stdout: String::from_utf8_lossy(&output.stdout).into(),
                    stderr: String::from_utf8_lossy(&output.stderr).into(),
                });
            }
            Err(err) => {
                return Err(CommandError::Io {
                    err,
                    command: substituted_cmd,
                })
            }
        },
    }
    Ok(())
}
