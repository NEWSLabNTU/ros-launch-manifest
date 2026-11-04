//! Phase 99 T1 — a consumer written against the pre-phase-99 names still
//! compiles this release: each old name is a `#[deprecated]` alias of the
//! renamed item, so a consumer can bump the tag before renaming. The aliases
//! go at the release that moves the model to schema 5; this file goes with
//! them.
#![allow(deprecated)]

use std::collections::BTreeMap;

use ros_launch_manifest_sched::{
    AssignRule, DEFAULT_TASK, DEFAULT_TIER, ResolvedTask, ResolvedTaskTable, ResolvedTier,
    ResolvedTierTable, SchedNode, TaskDef, TaskPlatformSpec, TierDef, TierPlatformSpec, resolve,
};

#[test]
fn the_old_names_are_the_new_items() {
    let head: TierDef = TierDef {
        posix: Some(TierPlatformSpec {
            priority: 80,
            stack_bytes: None,
            core: None,
            sched_class: None,
            preempt_threshold: None,
            deadline: None,
            budget: None,
            period: None,
            time_slice: None,
        }),
        ..Default::default()
    };
    let same: TaskDef = head.clone();
    let _: &TaskPlatformSpec = same.posix.as_ref().unwrap();
    assert_eq!(DEFAULT_TIER, DEFAULT_TASK);

    let tasks = BTreeMap::from([("hi".to_string(), head)]);
    let rules = [AssignRule {
        task: "hi".to_string(),
        nodes: vec!["/a".to_string()],
        scope: None,
    }];
    let nodes = [SchedNode {
        name: "/a".to_string(),
        scope: "/".to_string(),
    }];
    let table: ResolvedTierTable = resolve(&tasks, &rules, &nodes, "posix").unwrap();
    let new: &ResolvedTaskTable = &table;
    let first: &ResolvedTier = &new.tasks[0];
    let _: &ResolvedTask = first;
    assert_eq!(first.priority, 80);
    assert!(!table.is_single_tier());
    assert_eq!(table.is_single_tier(), table.is_single_task());
}
