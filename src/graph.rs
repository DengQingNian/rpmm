//! 启停事务的需求闭包与排序。
use crate::{Error, Result, config::Unit};
use std::collections::{BTreeMap, BTreeSet};

pub type Units = BTreeMap<String, Unit>;
pub struct Plan {
    pub layers: Vec<Vec<String>>,
    pub warnings: Vec<String>,
}

/// 计算启动事务。参数：units 为配置，roots 为请求对象。返回：并行层和弱依赖警告。
pub fn start_plan(units: &Units, roots: &[String]) -> Result<Plan> {
    let mut selected = BTreeSet::new();
    let mut todo = roots.to_vec();
    let mut warnings = vec![];
    while let Some(name) = todo.pop() {
        if !selected.insert(name.clone()) {
            continue;
        }
        let unit = units
            .get(&name)
            .ok_or_else(|| Error::Config(format!("缺少 unit：{name}")))?;
        for required in &unit.requires {
            if !units.contains_key(required) {
                return Err(Error::Config(format!(
                    "{name} 缺少 required unit：{required}"
                )));
            }
            todo.push(required.clone());
        }
        for wanted in &unit.wants {
            if units.contains_key(wanted) {
                todo.push(wanted.clone());
            } else {
                warnings.push(format!("{name} 缺少 wanted unit：{wanted}"));
            }
        }
    }
    Ok(Plan {
        layers: order(units, &selected)?,
        warnings,
    })
}

/// 对选中集合拓扑分层。参数：units 为定义，selected 为事务内名称。返回：排序层或环路诊断。
pub fn order(units: &Units, selected: &BTreeSet<String>) -> Result<Vec<Vec<String>>> {
    let mut edges: BTreeMap<String, BTreeSet<String>> = selected
        .iter()
        .map(|n| (n.clone(), BTreeSet::new()))
        .collect();
    for name in selected {
        let unit = units
            .get(name)
            .ok_or_else(|| Error::Config(format!("排序集合缺少 unit：{name}")))?;
        for dep in &unit.after {
            if selected.contains(dep) {
                edges.get_mut(name).unwrap().insert(dep.clone());
            }
        }
        for dep in &unit.before {
            if selected.contains(dep) {
                edges.get_mut(dep).unwrap().insert(name.clone());
            }
        }
    }
    let mut layers = vec![];
    while !edges.is_empty() {
        let layer: Vec<_> = edges
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(n, _)| n.clone())
            .collect();
        if layer.is_empty() {
            return Err(Error::Config(format!(
                "排序环路：{}",
                edges.keys().cloned().collect::<Vec<_>>().join(" -> ")
            )));
        }
        for name in &layer {
            edges.remove(name);
        }
        for deps in edges.values_mut() {
            for name in &layer {
                deps.remove(name);
            }
        }
        layers.push(layer);
    }
    Ok(layers)
}

/// 扩展显式停止到反向 Requires 闭包。参数：units 为实例快照，roots 为停止对象。返回：停止集合。
pub fn stop_set(units: &Units, roots: &[String]) -> BTreeSet<String> {
    let mut selected: BTreeSet<_> = roots.iter().cloned().collect();
    loop {
        let additions: Vec<_> = units
            .iter()
            .filter(|(n, u)| {
                !selected.contains(*n) && u.requires.iter().any(|d| selected.contains(d))
            })
            .map(|(n, _)| n.clone())
            .collect();
        if additions.is_empty() {
            break;
        }
        selected.extend(additions);
    }
    selected
}
