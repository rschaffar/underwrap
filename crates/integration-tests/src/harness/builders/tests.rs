use super::*;

fn synthetic_group(
    record: &mut libc::group,
    buffer: &mut [u8],
    result: &mut *mut libc::group,
) -> i32 {
    buffer[..8].copy_from_slice(b"fixture\0");
    record.gr_gid = 123;
    record.gr_name = buffer.as_mut_ptr().cast();
    *result = record;
    0
}

#[test]
fn private_group_lookup_copies_name_and_grows_only_for_bounded_erange() {
    let mut sizes = Vec::new();
    let name = group_name_with(123, |record, buffer, result| {
        sizes.push(buffer.len());
        if sizes.len() < 3 {
            libc::ERANGE
        } else {
            synthetic_group(record, buffer, result)
        }
    })
    .unwrap();
    assert_eq!(name, "fixture");
    assert_eq!(
        sizes,
        [
            GROUP_BUFFER_START,
            2 * GROUP_BUFFER_START,
            4 * GROUP_BUFFER_START
        ]
    );
    let mut calls = 0;
    assert!(
        group_name_with(123, |_, buffer, _| {
            calls += 1;
            assert!(buffer.len() <= GROUP_BUFFER_MAX);
            libc::ERANGE
        })
        .unwrap_err()
        .to_string()
        .contains("buffer limit")
    );
    assert_eq!(calls, 7);
    for code in [libc::EINTR, libc::EIO, libc::ENOENT] {
        let mut calls = 0;
        assert!(
            group_name_with(123, |_, _, _| {
                calls += 1;
                code
            })
            .is_err()
        );
        assert_eq!(calls, 1);
    }
}

#[test]
fn private_group_lookup_rejects_missing_invalid_record_and_name_without_identity_fallback() {
    assert!(
        group_name_with(123, |_, _, _| 0)
            .unwrap_err()
            .to_string()
            .contains("no NSS name")
    );
    for case in 0..8 {
        let error = group_name_with(123, |record, buffer, result| {
            synthetic_group(record, buffer, result);
            match case {
                0 => *result = std::ptr::dangling_mut(),
                1 => record.gr_gid = 124,
                2 => record.gr_name = std::ptr::null_mut(),
                3 => record.gr_name = std::ptr::dangling_mut(),
                4 => record.gr_name = buffer.as_mut_ptr().wrapping_add(buffer.len()).cast(),
                5 => buffer.fill(b'x'),
                6 => buffer[0] = 0,
                7 => buffer[0] = 0xff,
                _ => unreachable!(),
            }
            0
        });
        assert!(
            error.is_err(),
            "invalid group record case {case} was accepted"
        );
    }
}

#[test]
fn concurrent_effective_group_lookups_and_fixtures_preserve_identity() {
    let expected = current_group_name().unwrap();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let expected = &expected;
            scope.spawn(move || {
                for _ in 0..128 {
                    assert!(
                        current_group_name().unwrap() == *expected,
                        "effective-group lookup changed"
                    );
                }
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(async {
                    let fixture =
                        crate::harness::TestDaemon::start(std::collections::HashMap::new())
                            .await
                            .unwrap();
                    assert!(current_group_name().unwrap() == *expected);
                    fixture.stop().await.unwrap();
                });
            });
        }
    });
}

#[test]
fn wrapper_builder_creates_env_projection() {
    let wrapper = wrapper("/usr/bin/gh", "github/token", "GH_TOKEN");
    assert_eq!(wrapper.target, PathBuf::from("/usr/bin/gh"));
    assert_eq!(wrapper.resource, "github/token");
    assert!(wrapper.env);
    assert!(wrapper.files.is_none());
    assert!(!wrapper.libsecret);
}

#[test]
fn files_wrapper_builder_sets_path() {
    let w = files_wrapper("/usr/bin/true", "gws/state", ".config/gws");
    assert_eq!(w.resource, "gws/state");
    assert_eq!(
        w.files.as_ref().unwrap().paths,
        vec![".config/gws".to_string()]
    );
}

#[test]
fn files_wrapper_paths_builder_carries_all_paths() {
    let w = files_wrapper_paths(
        "/usr/bin/true",
        "gws/state",
        &[".config/gws", ".local/share/gws"],
    );
    assert_eq!(
        w.files.unwrap().paths,
        vec![".config/gws".to_string(), ".local/share/gws".to_string()],
    );
}
