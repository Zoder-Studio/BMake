use anyhow::{bail, Result};
use bmake_ast::Task;
use std::collections::{HashMap, HashSet};

/// Groups tasks into ordered "waves": tasks in the same wave have no
/// dependency between them and can run concurrently; each wave only starts
/// after every task it depends on has finished.
pub fn topological_waves(tasks: &[Task]) -> Result<Vec<Vec<usize>>> {
    let name_to_idx: HashMap<&str, usize> = tasks.iter().enumerate().map(|(i, t)| (t.name.as_str(), i)).collect();

    for task in tasks {
        for dep in &task.depends_on {
            if !name_to_idx.contains_key(dep.as_str()) {
                bail!("Task '{}' depends on unknown task '{}'", task.name, dep);
            }
        }
    }

    detect_cycle(tasks, &name_to_idx)?;

    let mut remaining: HashSet<usize> = (0..tasks.len()).collect();
    let mut done: HashSet<usize> = HashSet::new();
    let mut waves = Vec::new();

    while !remaining.is_empty() {
        let ready: Vec<usize> = remaining
            .iter()
            .copied()
            .filter(|&i| {
                tasks[i]
                    .depends_on
                    .iter()
                    .all(|d| done.contains(&name_to_idx[d.as_str()]))
            })
            .collect();

        if ready.is_empty() {
            bail!("Unable to resolve task dependency order (unexpected cycle)");
        }

        for &i in &ready {
            remaining.remove(&i);
            done.insert(i);
        }
        waves.push(ready);
    }

    Ok(waves)
}

/// Returns the subset of `tasks` needed to run `target`: the task itself
/// plus every task it transitively `Depends-on`. Used by
/// `bmake run <file> --task <name>`.
pub fn transitive_closure(tasks: &[Task], target: &str) -> Result<Vec<Task>> {
    let name_to_idx: HashMap<&str, usize> = tasks.iter().enumerate().map(|(i, t)| (t.name.as_str(), i)).collect();
    let Some(&target_idx) = name_to_idx.get(target) else {
        bail!("Task '{}' not found", target);
    };

    let mut needed: HashSet<usize> = HashSet::new();
    let mut stack = vec![target_idx];
    while let Some(idx) = stack.pop() {
        if !needed.insert(idx) {
            continue;
        }
        for dep in &tasks[idx].depends_on {
            if let Some(&dep_idx) = name_to_idx.get(dep.as_str()) {
                stack.push(dep_idx);
            }
        }
    }

    Ok(tasks
        .iter()
        .enumerate()
        .filter(|(i, _)| needed.contains(i))
        .map(|(_, t)| t.clone())
        .collect())
}

fn detect_cycle(tasks: &[Task], name_to_idx: &HashMap<&str, usize>) -> Result<()> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Unvisited,
        Visiting,
        Visited,
    }

    fn visit(
        i: usize,
        tasks: &[Task],
        name_to_idx: &HashMap<&str, usize>,
        state: &mut Vec<State>,
        path: &mut Vec<usize>,
    ) -> Result<()> {
        match state[i] {
            State::Visited => return Ok(()),
            State::Visiting => {
                let start = path.iter().position(|&p| p == i).unwrap_or(0);
                let mut chain: Vec<&str> = path[start..].iter().map(|&p| tasks[p].name.as_str()).collect();
                chain.push(tasks[i].name.as_str());
                bail!("BMake Error:\nCircular task dependency detected.\n\n{}", chain.join(" -> "));
            }
            State::Unvisited => {}
        }

        state[i] = State::Visiting;
        path.push(i);

        for dep in &tasks[i].depends_on {
            let dep_idx = name_to_idx[dep.as_str()];
            visit(dep_idx, tasks, name_to_idx, state, path)?;
        }

        path.pop();
        state[i] = State::Visited;
        Ok(())
    }

    let mut state = vec![State::Unvisited; tasks.len()];
    let mut path: Vec<usize> = Vec::new();

    for i in 0..tasks.len() {
        visit(i, tasks, name_to_idx, &mut state, &mut path)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmake_ast::Task;

    fn task(name: &str, deps: &[&str]) -> Task {
        Task {
            name: name.to_string(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn normal_dependency_orders_correctly() {
        let tasks = vec![task("Compile", &[]), task("Test", &["Compile"]), task("Package", &["Test"])];
        let waves = topological_waves(&tasks).unwrap();
        assert_eq!(waves.len(), 3);
        assert_eq!(tasks[waves[0][0]].name, "Compile");
        assert_eq!(tasks[waves[1][0]].name, "Test");
        assert_eq!(tasks[waves[2][0]].name, "Package");
    }

    #[test]
    fn independent_tasks_share_a_wave() {
        let tasks = vec![
            task("Compile", &[]),
            task("Test", &["Compile"]),
            task("Lint", &["Compile"]),
            task("Package", &["Test", "Lint"]),
        ];
        let waves = topological_waves(&tasks).unwrap();
        assert_eq!(waves[1].len(), 2);
    }

    #[test]
    fn circular_dependency_is_detected_with_chain() {
        let tasks = vec![task("A", &["B"]), task("B", &["C"]), task("C", &["A"])];
        let err = topological_waves(&tasks).unwrap_err();
        assert!(err.to_string().contains("Circular task dependency"));
    }

    #[test]
    fn unknown_dependency_is_rejected() {
        let tasks = vec![task("Package", &["Compile"])];
        let err = topological_waves(&tasks).unwrap_err();
        assert!(err.to_string().contains("unknown task"));
    }

    #[test]
    fn transitive_closure_includes_only_needed_tasks() {
        let tasks = vec![
            task("Compile", &[]),
            task("Test", &["Compile"]),
            task("Package", &["Test", "Compile"]),
            task("Deploy", &["Package"]),
        ];
        let subset = transitive_closure(&tasks, "Test").unwrap();
        let names: std::collections::HashSet<_> = subset.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains("Compile"));
        assert!(names.contains("Test"));
        assert!(!names.contains("Package"));
        assert!(!names.contains("Deploy"));
    }
}