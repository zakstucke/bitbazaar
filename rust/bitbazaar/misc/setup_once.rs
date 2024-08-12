use std::{future::Future, path::PathBuf, sync::LazyLock};

use crate::{misc::global_lock_host_async, prelude::*};

#[cfg(not(target_arch = "wasm32"))]
/// Clear all data for a given id.
pub async fn setup_once_clear(id: &str) -> RResult<(), AnyErr> {
    let workspace_dir = WORKSPACE_DIR_PARENT.join(id);
    if workspace_dir.exists() {
        tokio::fs::remove_dir_all(&workspace_dir)
            .await
            .change_context(AnyErr)?;
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
/// Get the outer path all data is stored in.
pub fn setup_once_storage_path() -> &'static std::path::Path {
    &WORKSPACE_DIR_PARENT
}

/// Not available on wasm, requires global_lock_host_async which is filesystem based.
#[cfg(not(target_arch = "wasm32"))]
/// Setup something once for a given version on the current host.
/// E.g. installing a package, downloading a file, etc.
/// - setup callback and otherwise callback passed a dedicated shared filespace for that version.
/// - no corrupt state, if setup fails, next call will definitely run setup again after emptying the folder.
/// - parallel/multi process safe, using a host-level global lock, setup will only be called once.
pub async fn setup_once<
    R,
    SetupFut: Future<Output = RResult<R, AnyErr>>,
    OtherwiseFut: Future<Output = RResult<R, AnyErr>>,
>(
    id: &str,
    version: &str,
    resetup_on_otherwise_error: bool,
    setup: impl FnOnce(PathBuf) -> SetupFut,
    otherwise: impl FnOnce(PathBuf) -> OtherwiseFut,
) -> RResult<R, AnyErr> {
    use crate::log::record_exception;

    let workspace_dir = WORKSPACE_DIR_PARENT.join(id).join(version);

    let success_flag_path = workspace_dir.join("bb_setup_once_success.txt");

    macro_rules! run_setup {
        () => {{
            tracing::info!(
                "setup_once: running setup for {} with version {}",
                version,
                id
            );

            // Delete all the existing contents in the folder in case a previous run failed:
            if workspace_dir.exists() {
                tokio::fs::remove_dir_all(&workspace_dir)
                    .await
                    .change_context(AnyErr)?;
            }

            // Create the workspace dir and any intermediary paths for the passed in dir if missing:
            tokio::fs::create_dir_all(&workspace_dir)
                .await
                .change_context(AnyErr)?;

            let value = setup(workspace_dir).await?;

            tokio::fs::File::create(success_flag_path)
                .await
                .change_context(AnyErr)?;

            Ok(value)
        }};
    }

    macro_rules! run_otherwise {
        () => {
            match otherwise(workspace_dir.clone()).await {
                Ok(value) => Ok(value),
                Err(err) => {
                    if resetup_on_otherwise_error {
                        record_exception("setup_once: otherwise() callback failed, resetup_on_otherwise_error=true so running setup() again.", format!("{:?}", err));
                        run_setup!()
                    } else {
                        Err(err)
                    }
                }
            }
        };
    }

    // Check if the success flag added to the folder, if it has then everything already done, otherwise can just be used.
    if success_flag_path.exists() {
        run_otherwise!()
    } else {
        // Setup needs running, there could be multiple calls getting here at the same time, hence the global lock needed,
        // then checking again for success before running setup.
        global_lock_host_async(&format!("{}_{}", id, version), async {
            if success_flag_path.exists() {
                run_otherwise!()
            } else {
                run_setup!()
            }
        })
        .await
    }
}

#[cfg(not(target_arch = "wasm32"))]
static WORKSPACE_DIR_PARENT: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::temp_dir().join(format!(
        "bitbazaar_{}_setup_once_storage",
        env!("CARGO_PKG_VERSION")
    ))
});

#[cfg(test)]
mod tests {
    use std::{
        sync::{atomic::AtomicUsize, Arc},
        time::Instant,
    };

    use super::*;

    use crate::testing::prelude::*;

    #[rstest]
    #[tokio::test]
    async fn test_setup_once_setup_single_setup_when_contended() -> RResult<(), AnyErr> {
        let id = "test_setup_once_setup_single_setup_when_contended";
        setup_once_clear(id).await?;

        let setup_num_calls = Arc::new(AtomicUsize::new(0));
        let setup = {
            let setup_num_calls = setup_num_calls.clone();
            |_workspace_dir: PathBuf| async move {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                setup_num_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            }
        };

        let otherwise_num_calls = Arc::new(AtomicUsize::new(0));
        let otherwise = {
            let otherwise_num_calls = otherwise_num_calls.clone();
            |_workspace_dir: PathBuf| async move {
                otherwise_num_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            }
        };

        let mut futs = vec![];
        for _ in 0..100 {
            futs.push(setup_once(
                id,
                "v1",
                false,
                setup.clone(),
                otherwise.clone(),
            ));
        }

        let elapsed = Instant::now();
        futures::future::try_join_all(futs).await?;
        let elapsed_millis = elapsed.elapsed().as_millis();
        assert!(elapsed_millis >= 100, "elapsed_millis: {}", elapsed_millis);
        assert!(elapsed_millis < 200, "elapsed_millis: {}", elapsed_millis);
        assert_eq!(
            setup_num_calls.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert_eq!(
            otherwise_num_calls.load(std::sync::atomic::Ordering::Relaxed),
            99
        );

        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_setup_once_resetup_on_otherwise_error() -> RResult<(), AnyErr> {
        let id = "test_setup_once_resetup_on_otherwise_error";
        setup_once_clear(id).await?;

        // When true, otherwise failure should trigger setup again a single time, if setup fails should error out:
        let setup_num_calls = Arc::new(AtomicUsize::new(0));
        let setup = {
            let setup_num_calls = setup_num_calls.clone();
            |_workspace_dir: PathBuf| async move {
                let num_calls = setup_num_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if num_calls == 0 || num_calls == 1 {
                    Ok(())
                } else {
                    Err(anyerr!())
                }
            }
        };
        let otherwise = |_workspace_dir: PathBuf| async move { Err(anyerr!()) };
        let ver = "v1";
        // First call setup succeeds
        setup_once(id, ver, true, setup.clone(), otherwise).await?;
        assert_eq!(
            setup_num_calls.load(std::sync::atomic::Ordering::Relaxed),
            1
        );

        // When =false, if otherwise fails should error straight away without running setup again:
        let result = setup_once(id, ver, false, setup.clone(), otherwise).await;
        assert!(result.is_err());
        assert_eq!(
            setup_num_calls.load(std::sync::atomic::Ordering::Relaxed),
            1
        );

        // Second otherwise should fail, triggering setup which should then succeed this second time:
        setup_once(id, ver, true, setup.clone(), otherwise).await?;
        assert_eq!(
            setup_num_calls.load(std::sync::atomic::Ordering::Relaxed),
            2
        );
        // Call again, otherwise should fail again, this time setup should fail too:
        let result = setup_once(id, ver, true, setup.clone(), otherwise).await;
        assert!(result.is_err());
        assert_eq!(
            setup_num_calls.load(std::sync::atomic::Ordering::Relaxed),
            3
        );

        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_setup_once_setup_recall_on_error_with_cleaned_workdir() -> RResult<(), AnyErr> {
        let id = "test_setup_once_setup_recall_on_error";
        setup_once_clear(id).await?;

        // Put something in the parent, so can make sure this isn't affected when the target workdir is cleared on error:
        tokio::fs::write(WORKSPACE_DIR_PARENT.join("parent_other_data.txt"), "test")
            .await
            .change_context(AnyErr)?;

        let setup_num_calls = Arc::new(AtomicUsize::new(0));
        for x in 0..3 {
            let result = setup_once(
                id,
                "v1",
                false,
                {
                    let setup_num_calls = setup_num_calls.clone();
                    |workspace_dir: PathBuf| async move {
                        let num_calls =
                            setup_num_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                        // Assert workspace is empty:
                        assert!(workspace_dir
                            .read_dir()
                            .change_context(AnyErr)?
                            .next()
                            .is_none());

                        // Add something to the workspace:
                        tokio::fs::write(workspace_dir.join("test.txt"), "test")
                            .await
                            .change_context(AnyErr)?;

                        if num_calls == 0 {
                            Err(anyerr!())
                        } else {
                            Ok(())
                        }
                    }
                },
                |_| async move { Ok(()) },
            )
            .await;
            // Setup should fail the first time:
            if x == 0 {
                assert!(result.is_err());
            } else {
                assert!(result.is_ok());
            }
        }

        // Should've been called twice, as failed the first time:
        assert_eq!(
            setup_num_calls.load(std::sync::atomic::Ordering::Relaxed),
            2
        );

        // Make sure the top level parent data wasn't affected by this clear:
        assert!(WORKSPACE_DIR_PARENT.join("parent_other_data.txt").exists());

        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_setup_once_workspace_dir_persistence() -> RResult<(), AnyErr> {
        let id = "test_setup_once_basic";
        let version = "v1";
        setup_once_clear(id).await?;

        let setup_num_calls = Arc::new(AtomicUsize::new(0));
        let setup = {
            let setup_num_calls = setup_num_calls.clone();
            |workspace_dir: PathBuf| async move {
                let file_path = workspace_dir.join("test.txt");
                tokio::fs::write(&file_path, "test")
                    .await
                    .change_context(AnyErr)?;
                setup_num_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(file_path)
            }
        };

        let otherwise_num_calls = Arc::new(AtomicUsize::new(0));
        let otherwise = {
            let otherwise_num_calls = otherwise_num_calls.clone();
            |workspace_dir: PathBuf| async move {
                let file_path = workspace_dir.join("test.txt");
                let content = tokio::fs::read_to_string(&file_path)
                    .await
                    .change_context(AnyErr)?;
                assert_eq!(content, "test");
                otherwise_num_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(file_path)
            }
        };
        let mut calls = vec![];
        for _ in 0..3 {
            calls.push(setup_once(id, version, false, setup.clone(), otherwise.clone()).await?);
        }
        // Setup should've been called once, otherwise twice:
        assert_eq!(
            setup_num_calls.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert_eq!(
            otherwise_num_calls.load(std::sync::atomic::Ordering::Relaxed),
            2
        );

        // All call values should be identical:
        for i in 1..calls.len() {
            assert_eq!(calls[i], calls[i - 1]);
        }

        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_setup_once_workspace_dir_changes_on_id_or_version() -> RResult<(), AnyErr> {
        let a_id = "i_a";
        let a_ver = "v_a";
        let b_id = "i_b";
        let b_ver = "v_b";
        setup_once_clear(a_id).await?;
        setup_once_clear(b_id).await?;

        let setup = |workspace_dir: PathBuf| async move { Ok(workspace_dir) };
        let otherwise = |workspace_dir: PathBuf| async move { Ok(workspace_dir) };
        assert_eq!(
            setup_once(a_id, a_ver, false, setup, otherwise).await?,
            setup_once(a_id, a_ver, false, setup, otherwise).await?
        );
        assert_ne!(
            setup_once(a_id, a_ver, false, setup, otherwise).await?,
            setup_once(b_id, a_ver, false, setup, otherwise).await?
        );
        assert_ne!(
            setup_once(a_id, a_ver, false, setup, otherwise).await?,
            setup_once(a_id, b_ver, false, setup, otherwise).await?
        );

        Ok(())
    }
}
