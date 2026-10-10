//! 桌面配置保存、冲突检测和候选图的回归验证。
use rpmm::{config, desktop, manager::Manager};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const BASE: &str =
    "[Service]\nExecStart=C:/fake.exe\nRestart=no\n[Install]\nWantedBy=multi-user.target\n";

/// 创建隔离测试目录。参数：无。返回：temp 下的测试目录。
fn root() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("temp/desktop-tests")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
    std::fs::create_dir_all(path.join("units")).unwrap();
    path
}

/// 验证保存失败不会写盘或替换已验证定义。参数：无。返回：无。
#[tokio::test]
async fn invalid_document_preserves_disk_and_generation() {
    let root = root();
    std::fs::write(root.join("units/app.service"), BASE).unwrap();
    let manager = Manager::new(&root).unwrap();
    assert!(
        manager
            .save_document(
                "app.service",
                "[Service]\nExecStart=relative.exe\n",
                Some(BASE)
            )
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("units/app.service")).unwrap(),
        BASE
    );
    assert_eq!(manager.status(None).unwrap()[0].config_version, 1);
    assert!(
        manager
            .save_document("new.service", "[Service]\nUnknown=true\n", None)
            .await
            .is_err()
    );
    assert!(!root.join("units/new.service").exists());
}

/// 验证编辑冲突与新建时的同名保护。参数：无。返回：无。
#[tokio::test]
async fn stale_editor_cannot_overwrite_other_changes() {
    let root = root();
    let manager = Manager::new(&root).unwrap();
    assert_eq!(
        manager
            .save_document("app.service", BASE, None)
            .await
            .unwrap(),
        2
    );
    let changed = BASE.replace("Restart=no", "Restart=always");
    std::fs::write(root.join("units/app.service"), &changed).unwrap();
    assert!(
        manager
            .save_document("app.service", BASE, Some(BASE))
            .await
            .is_err()
    );
    assert!(
        manager
            .save_document("app.service", BASE, None)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("units/app.service")).unwrap(),
        changed
    );
}

/// 验证新建、编辑覆盖文件与排序合并。参数：无。返回：无。
#[tokio::test]
async fn dropins_are_validated_and_merged_in_filename_order() {
    let root = root();
    let manager = Manager::new(&root).unwrap();
    manager
        .save_document("app.service", BASE, None)
        .await
        .unwrap();
    let earlier = "[Service]\nRestart=always\n";
    let later = "[Service]\nRestart=on-failure\n";
    manager
        .save_document("app.service.d/20-local.conf", later, None)
        .await
        .unwrap();
    manager
        .save_document("app.service.d/10-default.conf", earlier, None)
        .await
        .unwrap();
    assert_eq!(
        config::load(&root.join("units")).unwrap()["app.service"].restart,
        config::Restart::OnFailure
    );
    let replacement = "[Service]\nRestart=on-success\n";
    manager
        .save_document("app.service.d/20-local.conf", replacement, Some(later))
        .await
        .unwrap();
    assert_eq!(
        config::load(&root.join("units")).unwrap()["app.service"].restart,
        config::Restart::OnSuccess
    );
    let documents = desktop::documents(&root, "app.service").unwrap();
    assert_eq!(documents.len(), 3);
    assert!(
        documents
            .iter()
            .any(|document| document.text == replacement)
    );
    assert!(
        manager
            .save_document("missing.service.d/local.conf", later, None)
            .await
            .is_err()
    );
    assert!(!root.join("units/missing.service.d").exists());
}

/// 验证候选依赖环不落盘。参数：无。返回：无。
#[tokio::test]
async fn dependency_cycle_is_rejected_before_writing() {
    let root = root();
    let manager = Manager::new(&root).unwrap();
    manager
        .save_document("a.service", BASE, None)
        .await
        .unwrap();
    let b = format!("[Unit]\nAfter=a.service\n{BASE}");
    manager.save_document("b.service", &b, None).await.unwrap();
    let a = format!("[Unit]\nAfter=b.service\n{BASE}");
    assert!(
        manager
            .save_document("a.service", &a, Some(BASE))
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("units/a.service")).unwrap(),
        BASE
    );
}

/// 验证名称与大小限制阻止目录穿越及异常文档。参数：无。返回：无。
#[test]
fn document_paths_and_text_are_bounded() {
    for name in [
        "../app.service",
        "a.service.d/../local.conf",
        "a.service.d/C:/local.conf",
        "CON.service",
        "a.service.d/.conf",
        "a.service.d/sub/local.conf",
    ] {
        assert!(desktop::validate_document_name(name).is_err(), "{name}");
    }
    assert!(desktop::validate_text(&"x".repeat(desktop::MAX_DOCUMENT + 1)).is_err());
    assert!(desktop::validate_text("a\0b").is_err());
    let root = root();
    std::fs::write(
        root.join("oversized.txt"),
        "x".repeat(desktop::MAX_DOCUMENT + 1),
    )
    .unwrap();
    assert!(desktop::read_optional(&root.join("oversized.txt")).is_err());
}

/// 验证设置持久化并拒绝不合法刷新频率。参数：无。返回：无。
#[test]
fn preferences_roundtrip_and_validation() {
    let root = root();
    std::fs::create_dir_all(root.join("state")).unwrap();
    assert_eq!(
        desktop::load_preferences(&root).unwrap(),
        desktop::Preferences::default()
    );
    let preferences = desktop::Preferences {
        start_hidden: true,
        refresh_ms: 2000,
    };
    desktop::save_preferences(&root, &preferences).unwrap();
    assert_eq!(desktop::load_preferences(&root).unwrap(), preferences);
    assert!(
        desktop::save_preferences(
            &root,
            &desktop::Preferences {
                start_hidden: false,
                refresh_ms: 1
            }
        )
        .is_err()
    );
    assert_eq!(desktop::load_preferences(&root).unwrap(), preferences);
}

/// 验证原子替换失败后清理候选文件，避免污染后续配置重载。参数：无。返回：无。
#[test]
fn failed_atomic_replace_cleans_pending_file() {
    let root = root();
    let target = root.join("units/blocked.service");
    std::fs::create_dir(&target).unwrap();
    assert!(rpmm::manager::atomic_write(&target, BASE.as_bytes()).is_err());
    assert!(target.is_dir());
    assert_eq!(std::fs::read_dir(root.join("units")).unwrap().count(), 1);
}

/// 构建单服务配置包。参数：name 为名称，text 为正文，enabled 为启用状态。返回：版本一配置包。
fn bundle(name: &str, text: &str, enabled: bool) -> desktop::ConfigBundle {
    desktop::ConfigBundle {
        format: "rpmm-config".into(),
        version: 1,
        services: vec![desktop::ServiceBundle {
            name: name.into(),
            enabled,
            documents: vec![desktop::Document {
                name: name.into(),
                text: text.into(),
            }],
        }],
    }
}

/// 验证下次启动目录选择独立持久化并拒绝相对路径，全部数据隔离在 temp。参数：无。返回：无。
#[test]
fn data_directory_selection_roundtrip() {
    let default = root();
    let target = root();
    assert_eq!(desktop::load_root_selection(&default).unwrap(), default);
    desktop::save_root_selection(&default, &target).unwrap();
    assert_eq!(desktop::load_root_selection(&default).unwrap(), target);
    assert!(desktop::save_root_selection(&default, std::path::Path::new("relative")).is_err());
    assert_eq!(desktop::load_root_selection(&default).unwrap(), target);
    std::fs::write(default.join("state/data-root.json"), "\"relative\"").unwrap();
    assert!(desktop::load_root_selection(&default).is_err());
}

/// 验证多服务包、依赖和覆盖文件可以跨目录往返，启用状态保持一致。参数：无。返回：无。
#[tokio::test]
async fn config_bundle_roundtrip() {
    let source = root();
    let manager = Manager::new(&source).unwrap();
    manager
        .save_document("db.service", BASE, None)
        .await
        .unwrap();
    manager
        .save_document(
            "app.service",
            &format!("[Unit]\nRequires=db.service\nAfter=db.service\n{BASE}"),
            None,
        )
        .await
        .unwrap();
    manager
        .save_document(
            "app.service.d/20-local.conf",
            "[Service]\nRestart=always\nMemoryMax=512M\nCPUQuota=25%\n",
            None,
        )
        .await
        .unwrap();
    manager.enable("app.service", true).await.unwrap();
    let exported = desktop::export_bundle(&source, &manager.status(None).unwrap()).unwrap();
    let json = serde_json::to_vec(&exported).unwrap();
    let target = root();
    let restored = Manager::new(&target).unwrap();
    restored
        .import_bundle(serde_json::from_slice(&json).unwrap(), false)
        .await
        .unwrap();
    let unit = restored.unit_snapshot("app.service").unwrap();
    assert_eq!(unit.restart, config::Restart::Always);
    assert_eq!(unit.memory_max, Some(512 * 1024 * 1024));
    assert_eq!(unit.cpu_quota, Some(25));
    assert!(restored.status(Some("app.service")).unwrap()[0].enabled);
    assert_eq!(
        desktop::documents(&source, "app.service")
            .unwrap()
            .iter()
            .map(|d| (&d.name, &d.text))
            .collect::<Vec<_>>(),
        desktop::documents(&target, "app.service")
            .unwrap()
            .iter()
            .map(|d| (&d.name, &d.text))
            .collect::<Vec<_>>()
    );
}

/// 验证冲突默认拒绝，明确替换会移除包内未包含的旧覆盖文件。参数：无。返回：无。
#[tokio::test]
async fn bundle_conflict_and_replacement() {
    let root = root();
    let manager = Manager::new(&root).unwrap();
    manager
        .save_document("app.service", BASE, None)
        .await
        .unwrap();
    manager
        .save_document(
            "app.service.d/stale.conf",
            "[Service]\nRestart=always\n",
            None,
        )
        .await
        .unwrap();
    assert!(
        manager
            .import_bundle(bundle("app.service", BASE, false), false)
            .await
            .is_err()
    );
    assert!(root.join("units/app.service.d/stale.conf").exists());
    manager
        .import_bundle(bundle("app.service", BASE, false), true)
        .await
        .unwrap();
    assert!(!root.join("units/app.service.d/stale.conf").exists());
    assert_eq!(
        manager.unit_snapshot("app.service").unwrap().restart,
        config::Restart::No
    );
}

/// 验证导入整包先校验，错误图、路径穿越和版本均不会部分落盘。参数：无。返回：无。
#[tokio::test]
async fn invalid_bundles_never_partially_commit() {
    let root = root();
    let manager = Manager::new(&root).unwrap();
    let mut invalid = bundle("a.service", BASE, false);
    invalid.services.push(desktop::ServiceBundle {
        name: "b.service".into(),
        enabled: false,
        documents: vec![desktop::Document {
            name: "b.service".into(),
            text: "[Unit]\nRequires=missing.service\n[Service]\nExecStart=C:/app.exe\n".into(),
        }],
    });
    assert!(manager.import_bundle(invalid, false).await.is_err());
    assert!(!root.join("units/a.service").exists());
    let mut invalid = bundle("a.service", BASE, false);
    invalid.services[0].documents[0].name = "../a.service".into();
    assert!(manager.import_bundle(invalid, false).await.is_err());
    let mut invalid = bundle("a.service", BASE, false);
    invalid.version = 2;
    assert!(manager.import_bundle(invalid, false).await.is_err());
    assert!(manager.status(None).unwrap().is_empty());
}

/// 验证导入写盘失败恢复已修改的主配置，运行定义不变。参数：无。返回：无。
#[tokio::test]
#[cfg(windows)]
async fn bundle_disk_failure_rolls_back() {
    let root = root();
    let manager = Manager::new(&root).unwrap();
    manager
        .save_document("app.service", BASE, None)
        .await
        .unwrap();
    manager
        .save_document("z.service", BASE, None)
        .await
        .unwrap();
    // 允许读原文但拒绝删除第二份配置，确保第一份已写入后提交才失败。
    use std::os::windows::fs::OpenOptionsExt;
    let _blocked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(root.join("units/z.service"))
        .unwrap();
    let mut imported = bundle(
        "app.service",
        &BASE.replace("Restart=no", "Restart=always"),
        false,
    );
    imported.services.extend(
        bundle(
            "z.service",
            &BASE.replace("Restart=no", "Restart=always"),
            false,
        )
        .services,
    );
    let error = manager
        .import_bundle(imported, true)
        .await
        .unwrap_err()
        .to_string();
    assert!(!error.contains("恢复配置失败"));
    assert_eq!(
        std::fs::read_to_string(root.join("units/app.service")).unwrap(),
        BASE
    );
    assert_eq!(
        manager.unit_snapshot("app.service").unwrap().restart,
        config::Restart::No
    );
}

/// 验证删除服务会移除主配置、覆盖目录与启用状态，且不影响其他服务。参数：无。返回：无。
#[tokio::test]
async fn delete_service_removes_documents_and_enabled() {
    let root = root();
    let manager = Manager::new(&root).unwrap();
    manager
        .save_document("app.service", BASE, None)
        .await
        .unwrap();
    manager
        .save_document(
            "app.service.d/20-local.conf",
            "[Service]\nRestart=always\n",
            None,
        )
        .await
        .unwrap();
    manager
        .save_document("keep.service", BASE, None)
        .await
        .unwrap();
    manager.enable("app.service", true).await.unwrap();
    // 外部修改后使用旧正文删除会被拒绝，先确认冲突保护不落盘。
    let changed = BASE.replace("Restart=no", "Restart=always");
    std::fs::write(root.join("units/app.service"), &changed).unwrap();
    assert!(
        manager
            .delete_service("app.service", false, Some(BASE))
            .await
            .is_err()
    );
    assert!(root.join("units/app.service").exists());
    assert_eq!(
        manager
            .delete_service("app.service", false, Some(&changed))
            .await
            .unwrap(),
        5
    );
    assert!(!root.join("units/app.service").exists());
    assert!(!root.join("units/app.service.d").exists());
    assert!(root.join("units/keep.service").exists());
    let statuses = manager.status(None).unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].name, "keep.service");
    assert!(manager.status(Some("app.service")).is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("state/enabled.json"))
            .unwrap()
            .trim(),
        "[]"
    );
}

/// 验证被引用和缺失的服务不能删除，解除引用后可以删除。参数：无。返回：无。
#[tokio::test]
async fn delete_service_refuses_referenced_and_missing_units() {
    let root = root();
    let manager = Manager::new(&root).unwrap();
    manager
        .save_document("db.service", BASE, None)
        .await
        .unwrap();
    manager
        .save_document(
            "app.service",
            &format!("[Unit]\nRequires=db.service\nAfter=db.service\n{BASE}"),
            None,
        )
        .await
        .unwrap();
    let error = manager
        .delete_service("db.service", false, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("app.service") && error.contains("Requires"));
    assert!(root.join("units/db.service").exists());
    assert!(
        manager
            .delete_service("missing.service", false, None)
            .await
            .is_err()
    );
    std::fs::write(root.join("units/app.service"), BASE).unwrap();
    manager
        .delete_service("db.service", false, None)
        .await
        .unwrap();
    assert!(!root.join("units/db.service").exists());
    assert_eq!(manager.status(None).unwrap().len(), 1);
}

/// 验证删除过程中删盘失败会恢复已删除文件，运行定义不变。参数：无。返回：无。
#[tokio::test]
#[cfg(windows)]
async fn delete_service_disk_failure_rolls_back() {
    let root = root();
    let manager = Manager::new(&root).unwrap();
    manager
        .save_document("app.service", BASE, None)
        .await
        .unwrap();
    let dropin = "[Service]\nRestart=always\n";
    manager
        .save_document("app.service.d/20-local.conf", dropin, None)
        .await
        .unwrap();
    // 主配置先被删除，随后覆盖文件因共享模式拒绝删除，触发恢复。
    use std::os::windows::fs::OpenOptionsExt;
    let _blocked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(root.join("units/app.service.d/20-local.conf"))
        .unwrap();
    let error = manager
        .delete_service("app.service", false, Some(BASE))
        .await
        .unwrap_err()
        .to_string();
    assert!(!error.contains("恢复配置失败"));
    assert_eq!(
        std::fs::read_to_string(root.join("units/app.service")).unwrap(),
        BASE
    );
    assert_eq!(
        std::fs::read_to_string(root.join("units/app.service.d/20-local.conf")).unwrap(),
        dropin
    );
    assert_eq!(manager.status(None).unwrap().len(), 1);
}
