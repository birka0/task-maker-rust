use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Error};
use itertools::Itertools;
use regex::Regex;

use task_maker_dag::File;
use task_maker_diagnostics::Diagnostic;

use crate::ioi::sanity_checks::check_missing_graders;
use crate::ioi::{IOITask, TaskType, TestcaseId};
use crate::sanity_checks::SanityCheck;
use crate::{list_files, EvaluationData, UISender};

/// Check that all the graders are present inside att.
#[derive(Debug, Default)]
pub struct AttGraders;

impl SanityCheck<IOITask> for AttGraders {
    fn name(&self) -> &'static str {
        "AttGraders"
    }

    fn pre_hook(&mut self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        check_missing_graders(task, eval, "att")
    }
}

/// Check that all the templates are present inside att.
#[derive(Debug, Default)]
pub struct AttTemplates;

impl SanityCheck<IOITask> for AttTemplates {
    fn name(&self) -> &'static str {
        "AttTemplates"
    }

    fn pre_hook(&mut self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        for grader in task.grader_map.all_paths() {
            let ext = grader
                .extension()
                .ok_or_else(|| anyhow!("Grader has no extension"))?
                .to_string_lossy();
            let att_name = format!("att/{}.{}", task.name, ext);
            let template = task.path.join(&att_name);
            if !template.exists() {
                let grader_name = task.path_of(grader);
                eval.add_diagnostic(
                    Diagnostic::warning(format!("Missing template at {}", att_name))
                        .with_note(format!("Because of {}", grader_name.display())),
                )?;
            }
        }
        Ok(())
    }
}

/// Check that the sample cases inside att are valid symlinks.
#[derive(Debug, Default)]
pub struct AttSampleFiles;

impl SanityCheck<IOITask> for AttSampleFiles {
    fn name(&self) -> &'static str {
        "AttSampleFiles"
    }

    fn post_hook(&mut self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        let mut no_sample = true;
        for sample in list_files(&task.path, vec!["att/*input*.txt", "att/*output*.txt"]) {
            no_sample = false;
            // check if the file is a symlink
            if let Ok(content) = sample.read_link() {
                // check if the symlink is broken
                if sample.canonicalize().is_err() {
                    eval.add_diagnostic(
                        Diagnostic::error(format!(
                            "Sample case {} is a broken link",
                            task.path_of(&sample).display()
                        ))
                        .with_note(format!("It points to {}", content.display())),
                    )?;
                }
            } else {
                eval.add_diagnostic(Diagnostic::warning(format!(
                    "Sample case {} is not a symlink",
                    task.path_of(&sample).display()
                )).with_help("Move this file in the statement folder and symlink it here. This way the sample file can be included in the compiled statement."))?;
            }
        }
        if no_sample {
            eval.add_diagnostic(Diagnostic::warning("No sample file in att/"))?;
        }
        Ok(())
    }
}

/// Check that the input files inside the att folder are valid, the solution doesn't crash with them
/// and the sample output files score full score.
#[derive(Debug, Default)]
pub struct AttSampleFilesValid;

impl SanityCheck<IOITask> for AttSampleFilesValid {
    fn name(&self) -> &'static str {
        "AttSampleFilesValid"
    }

    fn pre_hook(&mut self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        let validator = &task.input_validator;
        let task_type = if let TaskType::Batch(data) = &task.task_type {
            data
        } else {
            return Ok(());
        };
        let official_solution = &task_type.output_generator;
        let samples = get_sample_files(task, eval).context("Failed to get sample files")?;
        for (input, output) in samples {
            let input_name = task.path_of(&input).to_owned();
            let input_handle = File::new(format!("Sample input file at {}", input_name.display()));
            let input_uuid = input_handle.uuid;
            eval.dag
                .provide_file(input_handle, input)
                .context("Failed to provide sample input file")?;

            // validate the input file
            let (val_handle, val) = validator
                .validate(
                    eval,
                    format!("Validation of sample case {}", input_name.display()),
                    0,
                    0,
                    input_uuid,
                )
                .context("Failed to validate sample input file")?;
            if let Some(mut val) = val {
                let input_name = input_name.clone();
                let sender = eval.sender.clone();
                val.capture_stderr(1024);
                eval.dag.on_execution_done(&val.uuid, move |res| {
                    if !res.status.is_success() {
                        let mut diagnostic = Diagnostic::error(format!(
                            "Sample input file {} is not valid",
                            input_name.display()
                        ))
                        .with_note(format!("The validator failed with: {:?}", res.status));
                        if let Some(stderr) = res.stderr {
                            diagnostic = diagnostic
                                .with_help("The validator stderr is:")
                                .with_help_attachment(stderr);
                        }
                        sender.add_diagnostic(diagnostic)?;
                    }
                    Ok(())
                });
                eval.dag.add_execution(val);
            }

            if let Some(solution) = &official_solution {
                let output_name = task.path_of(&output).to_owned();
                let output_handle =
                    File::new(format!("Sample output file at {}", output_name.display()));
                let output_uuid = output_handle.uuid;
                eval.dag
                    .provide_file(output_handle, output)
                    .context("Failed to provide sample output file")?;

                // generate the output file
                let (correct_output, sol) = solution
                    .generate(
                        task,
                        eval,
                        format!(
                            "Generation of output file relative to {}",
                            input_name.display()
                        ),
                        0,
                        0,
                        input_uuid,
                        val_handle,
                    )
                    .context("Failed to generate correct sample output file")?;
                let correct_output =
                    correct_output.ok_or_else(|| anyhow!("Missing official solution"))?;
                if let Some(mut sol) = sol {
                    sol.capture_stderr(1024);
                    let sender = eval.sender.clone();
                    eval.dag.on_execution_done(&sol.uuid, move |res| {
                        if !res.status.is_success() {
                            let mut diagnostic = Diagnostic::error(format!(
                                "Solution failed on sample input file {}",
                                input_name.display()
                            ))
                            .with_note(format!("The solution failed with: {:?}", res.status));
                            if let Some(stderr) = res.stderr {
                                diagnostic = diagnostic
                                    .with_help("The solution stderr is:")
                                    .with_help_attachment(stderr);
                            }
                            sender.add_diagnostic(diagnostic)?;
                        }
                        Ok(())
                    });
                    eval.dag.add_execution(sol);
                }

                // validate the output with the correct one
                let sender = eval.sender.clone();
                let chk = task_type
                    .checker
                    .check(
                        eval,
                        0,
                        format!("Checking sample output {}", output_name.display()),
                        input_uuid,
                        correct_output,
                        output_uuid,
                        move |score, message| {
                            if abs_diff_ne!(score, 1.0) {
                                sender.add_diagnostic(Diagnostic::warning(format!(
                                    "Sample output file {} scores {}: {}",
                                    output_name.display(),
                                    score,
                                    message
                                )))?;
                            }
                            Ok(())
                        },
                    )
                    .context("Failed to check sample files")?;
                eval.dag.add_execution(chk);
            }
        }
        Ok(())
    }
}

/// Search the input-output sample pairs inside the att folder. Returns a list of (input,output)
/// pairs with their numbers matching.
fn get_sample_files(
    task: &IOITask,
    eval: &mut EvaluationData,
) -> Result<Vec<(PathBuf, PathBuf)>, Error> {
    let regex = Regex::new(r"(in|out)put(\d+)\.txt$").unwrap();
    let extract_num = |path: &PathBuf| {
        let path_str = path.to_string_lossy();
        let caps = regex.captures(path_str.as_ref());
        if let Some(caps) = caps {
            if let Some(num) = caps.get(2) {
                let num: TestcaseId = num.as_str().parse().ok()?;
                return Some(num);
            }
        }
        None
    };
    let mut inputs: HashMap<_, Vec<_>> = HashMap::new();
    for input in list_files(&task.path, vec!["att/*input*.txt"]) {
        if let Some(num) = extract_num(&input) {
            inputs.entry(num).or_default().push(input);
        }
    }
    for (num, files) in inputs.iter().sorted() {
        if files.len() == 1 {
            continue;
        }
        let paths = files
            .iter()
            .map(|p| task.path_of(p).to_string_lossy())
            .join(", ");
        eval.add_diagnostic(
            Diagnostic::error(format!("Sample input {} is present more than once", num))
                .with_note(format!("Found at: {}", paths)),
        )?;
    }
    let mut outputs: HashMap<_, Vec<_>> = HashMap::new();
    for output in list_files(&task.path, vec!["att/*output*.txt"]) {
        if let Some(num) = extract_num(&output) {
            outputs.entry(num).or_default().push(output);
        }
    }
    for (num, files) in outputs.iter().sorted() {
        if files.len() == 1 {
            continue;
        }
        let paths = files
            .iter()
            .map(|p| task.path_of(p).to_string_lossy())
            .join(", ");
        eval.add_diagnostic(
            Diagnostic::error(format!("Sample output {} is present more than once", num))
                .with_note(format!("Found at: {}", paths)),
        )?;
    }
    let mut samples = Vec::new();
    for (num, inputs) in inputs {
        let output = if let Some(output) = outputs.remove(&num) {
            output[0].clone()
        } else {
            eval.add_diagnostic(Diagnostic::error(format!(
                "Sample input file {} does not have its output file",
                task.path_of(&inputs[0]).display()
            )))?;
            continue;
        };
        samples.push((inputs[0].clone(), output));
    }
    for (_, outputs) in outputs {
        eval.add_diagnostic(Diagnostic::error(format!(
            "Sample output file {} does not have its input file",
            task.path_of(&outputs[0]).display()
        )))?;
    }
    Ok(samples)
}
