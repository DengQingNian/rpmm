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
