use anyhow::Error;
use itertools::Itertools;

use crate::sanity_checks::SanityCheck;
use crate::{EvaluationData, IOITask, UISender};

/// Check that all the subtasks have a name.
#[derive(Debug, Default)]
pub struct MissingSubtaskNames;

impl SanityCheck<IOITask> for MissingSubtaskNames {
    fn name(&self) -> &'static str {
        "MissingSubtaskNames"
    }

    fn pre_hook(&mut self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        let mut missing_name = vec![];
        for subtask_id in task.subtasks.keys().sorted() {
            let subtask = &task.subtasks[subtask_id];
            if subtask.name.is_none() {
                missing_name.push(format!(
                    "Subtask {} ({} points)",
                    subtask.id, subtask.max_score
                ));
            }
        }
        if !missing_name.is_empty() {
            eval.sender.send_warning(format!(
                "These subtasks are missing a name (use '#STNAME: name' in gen/GEN): {}",
                missing_name.join(", ")
            ))?;
        }
        Ok(())
    }
}

/// Check that all the solutions (that are not symlinks) contain at least one check.
#[derive(Debug, Default)]
pub struct SolutionsWithNoChecks;

impl SanityCheck<IOITask> for SolutionsWithNoChecks {
    fn name(&self) -> &'static str {
        "SolutionsWithNoChecks"
    }

    fn pre_hook(&mut self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        for subtask in task.subtasks.values() {
            if subtask.name.is_none() {
                // If not all the subtasks have a name, do not bother with the solutions, it's much
                // more important to give everything a name before.
                return Ok(());
            }
        }

        let mut solutions = vec![];
        for solution in eval.solutions.iter() {
            if !solution.checks.is_empty() {
                continue;
            }
            let path = &solution.source_file.path;
            // Ignore the symlinks, since they may come from att/, in which we don't want to put the
            // checks.
            if path.is_symlink() {
                continue;
            }
            solutions.push(format!(
                "{}",
                solution.source_file.relative_path().display()
            ))
        }
        if !solutions.is_empty() {
            eval.sender.send_warning(format!(
                "The following solutions are missing the subtask checks: {} (try running task-maker-tools add-solution-checks)",
                solutions.join(", ")
            ))?;
        }
        Ok(())
    }
}

/// Check that all the checks target at least one subtask.
#[derive(Debug, Default)]
pub struct InvalidSubtaskName;

impl SanityCheck<IOITask> for InvalidSubtaskName {
    fn name(&self) -> &'static str {
        "InvalidSubtaskName"
    }

    fn pre_hook(&mut self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        let subtask_names = task
            .subtasks
            .keys()
            .sorted()
            .filter_map(|st| task.subtasks[st].name.as_ref())
            .join(", ");
        for solution in &eval.solutions {
            for check in &solution.checks {
                let subtasks = task.find_subtasks_by_pattern_name(&check.subtask_name_pattern);
                if subtasks.is_empty() {
                    eval.sender.send_error(format!(
                        "Invalid subtask name '{}' in solution '{}' (valid names are: {})",
                        check.subtask_name_pattern,
                        solution.source_file.relative_path().display(),
                        subtask_names
                    ))?;
                }
            }
        }
        Ok(())
    }
}
