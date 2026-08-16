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