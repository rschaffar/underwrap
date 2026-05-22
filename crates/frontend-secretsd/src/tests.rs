use super::*;

#[test]
fn parse_args_from_parses_required_and_optional_arguments() {
    let args = vec![
        "underwrap-secretsd".to_string(),
        "--bus-address".to_string(),
        "unix:path=/tmp/bus".to_string(),
        "--init-fd".to_string(),
        "5".to_string(),
        "--state-path".to_string(),
        "/tmp/.underwrap-ns-bus/libsecret-state.jcs".to_string(),
        "--ready-fd".to_string(),
        "9".to_string(),
    ];
    let parsed = parse_args_from(&args).unwrap();
    assert_eq!(parsed.bus_address, "unix:path=/tmp/bus");
    assert_eq!(parsed.init_fd, 5);
    assert_eq!(
        parsed.state_path,
        PathBuf::from("/tmp/.underwrap-ns-bus/libsecret-state.jcs")
    );
    assert_eq!(parsed.ready_fd, Some(9));
}

#[test]
fn parse_args_from_requires_known_arguments_and_values() {
    let err = parse_args_from(&["underwrap-secretsd".to_string()]).unwrap_err();
    assert!(err.to_string().contains("--bus-address is required"));

    let err = parse_args_from(&[
        "underwrap-secretsd".to_string(),
        "--bus-address".to_string(),
        "unix:path=/tmp/bus".to_string(),
        "--init-fd".to_string(),
    ])
    .unwrap_err();
    assert!(err.to_string().contains("--init-fd requires a value"));

    let err = parse_args_from(&[
        "underwrap-secretsd".to_string(),
        "--bus-address".to_string(),
        "unix:path=/tmp/bus".to_string(),
        "--init-fd".to_string(),
        "nope".to_string(),
        "--state-path".to_string(),
        "/tmp/x".to_string(),
    ])
    .unwrap_err();
    assert!(err.to_string().contains("--init-fd must be a number"));

    let err = parse_args_from(&[
        "underwrap-secretsd".to_string(),
        "--bus-address".to_string(),
        "unix:path=/tmp/bus".to_string(),
        "--init-fd".to_string(),
        "5".to_string(),
    ])
    .unwrap_err();
    assert!(err.to_string().contains("--state-path is required"));

    let err =
        parse_args_from(&["underwrap-secretsd".to_string(), "--bogus".to_string()]).unwrap_err();
    assert!(err.to_string().contains("unknown argument"));
}
