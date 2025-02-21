use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Error};

use task_maker_dag::{Execution, ExecutionCommand, File};
use task_maker_diagnostics::Diagnostic;

use crate::UISender;
use crate::{bind_exec_callbacks, ui::UIMessage, EvaluationData, Tag};

pub struct AsyFile;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AsyDependency {
    pub sandbox_path: PathBuf,
    pub local_path: PathBuf,
}

impl AsyFile {
    /// Compile the asy file and return the handle to the compiled and cropped pdf file.
    pub fn compile<P: Into<PathBuf>>(
        source: P,
        eval: &mut EvaluationData,
        booklet_name: &str,
    ) -> Result<File, Error> {
        let source_path = source.into();
        let booklet = booklet_name.to_string();
        let name = source_path
            .file_name()
            .ok_or_else(|| anyhow!("Invalid path of asy file: {:?}", source_path))?
            .to_string_lossy()
            .to_string();
        let source_file = File::new(format!("Source of {}", name));

        let mut comp = Execution::new(
            format!("Compilation of {}", name),
            ExecutionCommand::system("asy"),
        );
        comp.args(vec![
            "-f",
            "pdf",
            "-o",
            "output.pdf",
            "-localhistory", // This prevents "failed to create directory /.asy."
            "source.asy",
        ]);
        comp.limits_mut()
            .read_only(false)
            .wall_time(10.0) // asy tends to deadlock on failure
            .stack(8192 * 1024) // due to a libgc bug, asy may crash with unlimited stack
            .allow_multiprocess()
            .add_extra_readable_dir("/etc")
            .mount_tmpfs(true)
            .mount_proc(true);
        comp.tag(Tag::Booklet.into());
        comp.input(&source_file, "source.asy", false);
        eval.dag
            .provide_file(source_file, &source_path)
            .context("Failed to provide any source file")?;
        bind_exec_callbacks!(
            eval,
            comp.uuid,
            |status, booklet, name| UIMessage::IOIBookletDependency {
                booklet,
                name,
                step: 0,
                num_steps: 2,
                status
            },
            booklet,
            name
        )?;
        let deps = AsyFile::find_asy_deps(&source_path).with_context(|| {
            format!(
                "Failed to find asy dependencies of {}",
                source_path.display()
            )
        })?;
        for dep in deps {
            let file = File::new(format!(
                "Dependency {} of {}",
                dep.sandbox_path.display(),
                name
            ));
            comp.input(&file, &dep.sandbox_path, false);
            eval.dag
                .provide_file(file, &dep.local_path)
                .context("Failed to provide asy dependency")?;
        }
        let compiled = comp.output("output.pdf");
        if eval.dag.data.config.copy_logs {
            let log_dir = eval.task_root.join("bin/logs/asy");
            let stderr_dest = log_dir.join(format!("{}.stderr.log", name));
            let stdout_dest = log_dir.join(format!("{}.stdout.log", name));
            eval.dag
                .write_file_to_allow_fail(comp.stderr(), stderr_dest, false);
            eval.dag
                .write_file_to_allow_fail(comp.stdout(), stdout_dest, false);
        }

        comp.capture_stderr(1024);
        eval.dag.on_execution_done(&comp.uuid, {
            let sender = eval.sender.clone();
            let name = name.clone();
            move |result| {
                if !result.status.is_success() {
                    let mut diagnostic = Diagnostic::error(format!("Failed to compile {}", name));
                    if result.status.is_internal_error() {
                        diagnostic = diagnostic.with_help("Is 'asymptote' installed?");
                    }
                    if let Some(stderr) = result.stderr {
                        diagnostic = diagnostic.with_help_attachment(stderr);
                    }
                    sender.add_diagnostic(diagnostic)?;
                }
                Ok(())
            }
        });
        eval.dag.add_execution(comp);

        let mut crop = Execution::new(
            format!("Crop of {}", name),
            ExecutionCommand::system("pdfcrop"),
        );
        crop.limits_mut()
            .read_only(false)
            .wall_time(10.0) // asy tends to deadlock on failure
            .allow_multiprocess()
            .add_extra_readable_dir("/etc")
            .mount_tmpfs(true);
        crop.tag(Tag::Booklet.into());
        crop.args(vec!["source.pdf"]);
        crop.input(compiled, "source.pdf", false);
        bind_exec_callbacks!(
            eval,
            crop.uuid,
            |status, booklet, name| UIMessage::IOIBookletDependency {
                booklet,
                name,
                step: 1,
                num_steps: 2,
                status
            },
            booklet,
            name
        )?;
        let cropped = crop.output("source-crop.pdf");
        crop.capture_stderr(1024);
        eval.dag.on_execution_done(&crop.uuid, {
            let sender = eval.sender.clone();
            move |result| {
                if !result.status.is_success() {
                    let mut diagnostic =
                        Diagnostic::error(format!("Failed to crop pdf of {}", name));
                    if result.status.is_internal_error() {
                        diagnostic = diagnostic.with_help("Is 'pdfcrop' installed?");
                    }
                    if let Some(stderr) = result.stderr {
                        diagnostic = diagnostic.with_help_attachment(stderr);
                    }
                    sender.add_diagnostic(diagnostic)?;
                }
                Ok(())
            }
        });
        eval.dag.add_execution(crop);

        Ok(cropped)
    }

    /// Search for all the dependencies of an Asymptote source file.
    ///
    /// This includes all the files in the source's directory.
    fn find_asy_deps(source_path: &Path) -> Result<Vec<AsyDependency>, Error> {
        let source_dir = source_path
            .parent()
            .ok_or_else(|| anyhow!("File {:?} does not have a parent", source_path))?;

        Ok(glob::glob(&format!("{}/**/*", source_dir.display()))
            .with_context(|| format!("failed to glob {}/**/*", source_dir.display()))?
            .filter_map(|p| p.ok())
            .filter(|p| p != source_path)
            .filter(|p| !p.is_dir())
            .map(|p| AsyDependency {
                sandbox_path: p.strip_prefix(source_dir).unwrap_or(&p).into(),
                local_path: p,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::fs::write;

    use speculoos::prelude::*;

    use super::*;

    #[test]
    fn test_find_asy_deps() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path();
        write(path.join("source.asy"), "contents").unwrap();
        write(path.join("util.asy"), "contents").unwrap();
        write(path.join("image.png"), "contents").unwrap();
        std::fs::create_dir_all(path.join("assets")).unwrap();
        write(path.join("assets/wow.txt"), "contents").unwrap();

        let deps = AsyFile::find_asy_deps(&path.join("source.asy")).unwrap();

        assert_that(&deps.len()).is_equal_to(3);
        let dep = AsyDependency {
            local_path: path.join("util.asy"),
            sandbox_path: PathBuf::from("util.asy"),
        };
        assert!(deps.contains(&dep), "{:#?} vs {:#?}", deps, dep);
        let dep = AsyDependency {
            local_path: path.join("image.png"),
            sandbox_path: PathBuf::from("image.png"),
        };
        assert!(deps.contains(&dep), "{:#?} vs {:#?}", deps, dep);
        let dep = AsyDependency {
            local_path: path.join("assets/wow.txt"),
            sandbox_path: PathBuf::from("assets/wow.txt"),
        };
        assert!(deps.contains(&dep), "{:#?} vs {:#?}", deps, dep);
    }
}
