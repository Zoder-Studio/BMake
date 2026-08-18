use anyhow::{bail, Result};
use bmake_ast::{BMakeFile, ValueNode};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug)]
pub struct UsageReport {
    pub used_paths: HashSet<String>,
}

/// Resolves `${{ Value.a.b.c }}` and `${{ Secret.name }}` references found
/// in Env, Command, and Dependency/Tool Need fields, replacing them in
/// place. `secrets` must already contain every secret name this file
/// references (see `referenced_secret_names`) — nothing is fetched lazily
/// here. Returns which Value paths were used (unused-value warning) and
/// which secret VALUES were substituted (so the executor can mask them).
pub fn resolve(file: &mut BMakeFile, secrets: &HashMap<String, String>) -> Result<(UsageReport, HashSet<String>)> {
    let mut used_values = HashSet::new();
    let mut used_secret_values = HashSet::new();
    let values = file.values.clone();

    for v in file.env.values_mut() {
        *v = resolve_string(v, &values, secrets, &mut used_values, &mut used_secret_values)?;
    }
    for dep in file.dependencies.iter_mut() {
        dep.need = resolve_string(&dep.need, &values, secrets, &mut used_values, &mut used_secret_values)?;
    }
    for tool in file.tools.iter_mut() {
        tool.need = resolve_string(&tool.need, &values, secrets, &mut used_values, &mut used_secret_values)?;
    }
    for task in file.tasks.iter_mut() {
        for v in task.env.values_mut() {
            *v = resolve_string(v, &values, secrets, &mut used_values, &mut used_secret_values)?;
        }
        for step in task.commands.iter_mut() {
            step.command = resolve_string(&step.command, &values, secrets, &mut used_values, &mut used_secret_values)?;
        }
    }

    Ok((UsageReport { used_paths: used_values }, used_secret_values))
}

/// Like `resolve`, but only substitutes `${{ Value.* }}` — `${{ Secret.* }}`
/// is left untouched. Used by `bmake validate`, which checks that
/// referenced secrets are at least *declared*, without requiring the vault
/// passphrase.
pub fn resolve_values_only(file: &mut BMakeFile) -> Result<UsageReport> {
    let mut used_values = HashSet::new();
    let values = file.values.clone();

    for v in file.env.values_mut() {
        *v = resolve_string_values_only(v, &values, &mut used_values)?;
    }
    for dep in file.dependencies.iter_mut() {
        dep.need = resolve_string_values_only(&dep.need, &values, &mut used_values)?;
    }
    for tool in file.tools.iter_mut() {
        tool.need = resolve_string_values_only(&tool.need, &values, &mut used_values)?;
    }
    for task in file.tasks.iter_mut() {
        for v in task.env.values_mut() {
            *v = resolve_string_values_only(v, &values, &mut used_values)?;
        }
        for step in task.commands.iter_mut() {
            step.command = resolve_string_values_only(&step.command, &values, &mut used_values)?;
        }
    }
    Ok(UsageReport { used_paths: used_values })
}

/// Every `Secret.<name>` referenced anywhere in the file, computed without
/// touching the vault — used to decide exactly which secrets to decrypt.
pub fn referenced_secret_names(file: &BMakeFile) -> HashSet<String> {
    let mut names = HashSet::new();
    for v in file.env.values() {
        collect_secret_refs(v, &mut names);
    }
    for d in &file.dependencies {
        collect_secret_refs(&d.need, &mut names);
    }
    for t in &file.tools {
        collect_secret_refs(&t.need, &mut names);
    }
    for task in &file.tasks {
        for v in task.env.values() {
            collect_secret_refs(v, &mut names);
        }
        for step in &task.commands {
            collect_secret_refs(&step.command, &mut names);
        }
    }
    names
}

fn collect_secret_refs(s: &str, out: &mut HashSet<String>) {
    let mut rest = s;
    while let Some(start) = rest.find("${{") {
        let after = &rest[start + 3..];
        let Some(end) = after.find("}}") else { break };
        let expr = after[..end].trim();
        if let Some(name) = expr.strip_prefix("Secret.") {
            out.insert(name.to_string());
        }
        rest = &after[end + 2..];
    }
}

/// Every dotted leaf path reachable in the Value tree (e.g.
/// "Project.Android.CompileSdk"), used to compute which ones went unused.
pub fn all_paths(values: &BTreeMap<String, ValueNode>) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_paths(values, String::new(), &mut out);
    out
}

fn collect_paths(map: &BTreeMap<String, ValueNode>, prefix: String, out: &mut HashSet<String>) {
    for (k, v) in map {
        let path = if prefix.is_empty() { k.clone() } else { format!("{}.{}", prefix, k) };
        match v {
            ValueNode::Scalar(_) => {
                out.insert(path);
            }
            ValueNode::Map(m) => collect_paths(m, path, out),
        }
    }
}

fn resolve_string(
    input: &str,
    values: &BTreeMap<String, ValueNode>,
    secrets: &HashMap<String, String>,
    used_values: &mut HashSet<String>,
    used_secret_values: &mut HashSet<String>,
) -> Result<String> {
    let mut out = String::new();
    let mut rest = input;

    while let Some(start) = rest.find("${{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        let Some(end) = after.find("}}") else {
            bail!("Unterminated reference in '{}': expected closing '}}}}'", input);
        };
        let expr = after[..end].trim();
        out.push_str(&resolve_expr(expr, values, secrets, used_values, used_secret_values)?);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn resolve_expr(
    expr: &str,
    values: &BTreeMap<String, ValueNode>,
    secrets: &HashMap<String, String>,
    used_values: &mut HashSet<String>,
    used_secret_values: &mut HashSet<String>,
) -> Result<String> {
    if let Some(name) = expr.strip_prefix("Secret.") {
        let Some(v) = secrets.get(name) else {
            bail!(
                "Secret \"{}\" was not found.\n\nCreate it with:\n\n    bmake add secret {}",
                name,
                name
            );
        };
        used_secret_values.insert(v.clone());
        return Ok(v.clone());
    }
    resolve_value_expr(expr, values, used_values)
}

fn resolve_string_values_only(input: &str, values: &BTreeMap<String, ValueNode>, used_values: &mut HashSet<String>) -> Result<String> {
    let mut out = String::new();
    let mut rest = input;

    while let Some(start) = rest.find("${{") {
        let after = &rest[start + 3..];
        let Some(end) = after.find("}}") else {
            bail!("Unterminated reference in '{}': expected closing '}}}}'", input);
        };
        let expr = after[..end].trim();
        out.push_str(&rest[..start]);
        if expr.starts_with("Secret.") {
            out.push_str("${{ ");
            out.push_str(expr);
            out.push_str(" }}");
        } else {
            out.push_str(&resolve_value_expr(expr, values, used_values)?);
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn resolve_value_expr(expr: &str, values: &BTreeMap<String, ValueNode>, used_values: &mut HashSet<String>) -> Result<String> {
    let Some(path) = expr.strip_prefix("Value.") else {
        bail!("Unknown reference '${{{{ {} }}}}' — expected 'Value.<path>' or 'Secret.<name>'", expr);
    };

    let segments: Vec<&str> = path.split('.').collect();
    let mut current = values;
    for (i, seg) in segments.iter().enumerate() {
        let Some(node) = current.get(*seg) else {
            bail!("Undefined value:\n\n    Value.{}\n\nNo such value was declared in Value.", path);
        };
        if i == segments.len() - 1 {
            return match node {
                ValueNode::Scalar(s) => {
                    used_values.insert(path.to_string());
                    Ok(s.clone())
                }
                ValueNode::Map(_) => bail!("'Value.{}' is a nested group, not a scalar value — reference a leaf key instead.", path),
            };
        }
        match node {
            ValueNode::Map(m) => current = m,
            ValueNode::Scalar(_) => bail!("Undefined value:\n\n    Value.{}\n\nNo such value was declared in Value.", path),
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmake_ast::Dependency;

    fn values_with_project_name() -> BTreeMap<String, ValueNode> {
        let mut project = BTreeMap::new();
        project.insert("Name".to_string(), ValueNode::Scalar("MyApp".to_string()));
        let mut root = BTreeMap::new();
        root.insert("Project".to_string(), ValueNode::Map(project));
        root
    }

    #[test]
    fn resolves_nested_scalar_reference() {
        let mut file = BMakeFile { values: values_with_project_name(), ..Default::default() };
        file.dependencies.push(Dependency { name: "X".into(), need: "${{ Value.Project.Name }}".into() });
        let (report, _) = resolve(&mut file, &HashMap::new()).unwrap();
        assert_eq!(file.dependencies[0].need, "MyApp");
        assert!(report.used_paths.contains("Project.Name"));
    }

    #[test]
    fn undefined_value_is_a_clear_error() {
        let mut file = BMakeFile { values: values_with_project_name(), ..Default::default() };
        file.env.insert("X".into(), "${{ Value.Project.Missing }}".into());
        let err = resolve(&mut file, &HashMap::new()).unwrap_err();
        assert!(err.to_string().contains("Undefined value"));
    }

    #[test]
    fn secret_reference_resolves_from_provided_map() {
        let mut file = BMakeFile::default();
        file.env.insert("TOKEN".into(), "${{ Secret.DeployToken }}".into());
        let mut secrets = HashMap::new();
        secrets.insert("DeployToken".to_string(), "s3cr3t".to_string());
        let (_, used_secret_values) = resolve(&mut file, &secrets).unwrap();
        assert_eq!(file.env.get("TOKEN").unwrap(), "s3cr3t");
        assert!(used_secret_values.contains("s3cr3t"));
    }

    #[test]
    fn missing_secret_is_a_clear_error() {
        let mut file = BMakeFile::default();
        file.env.insert("TOKEN".into(), "${{ Secret.Nope }}".into());
        let err = resolve(&mut file, &HashMap::new()).unwrap_err();
        assert!(err.to_string().contains("was not found"));
    }

    #[test]
    fn referenced_secret_names_finds_all_usages() {
        let mut file = BMakeFile::default();
        file.env.insert("A".into(), "${{ Secret.One }}".into());
        file.dependencies.push(Dependency { name: "d".into(), need: "${{ Secret.Two }}".into() });
        let names = referenced_secret_names(&file);
        assert!(names.contains("One"));
        assert!(names.contains("Two"));
    }

    #[test]
    fn resolve_values_only_leaves_secret_refs_untouched() {
        let mut file = BMakeFile::default();
        file.env.insert("X".into(), "${{ Secret.Foo }}".into());
        resolve_values_only(&mut file).unwrap();
        assert_eq!(file.env.get("X").unwrap(), "${{ Secret.Foo }}");
    }
}