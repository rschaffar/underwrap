use super::*;

#[test]
fn parse_args_from_parses_required_and_optional_arguments() {
    let args = vec![
        "underwrap-secretsd".into(),
        "--bus-address".into(),
        "unix:path=/tmp/bus".into(),
        "--init-fd".into(),
        "5".into(),
        "--publication-fd".into(),
        "8".into(),
        "--ready-fd".into(),
        "9".into(),
    ];
    let parsed = parse_args_from(&args).unwrap();
    assert_eq!(parsed.bus_address, "unix:path=/tmp/bus");
    assert_eq!(parsed.init_fd, 5);
    assert_eq!(parsed.publication_fd, 8);
    assert_eq!(parsed.ready_fd, Some(9));
}

#[test]
fn parse_args_from_requires_known_arguments_and_values() {
    let err = parse_args_from(&["underwrap-secretsd".into()]).unwrap_err();
    assert!(err.to_string().contains("--bus-address is required"));
    let err = parse_args_from(&[
        "underwrap-secretsd".into(),
        "--bus-address".into(),
        "unix:path=/tmp/bus".into(),
        "--init-fd".into(),
        "5".into(),
    ])
    .unwrap_err();
    assert!(err.to_string().contains("--publication-fd is required"));
    let err = parse_args_from(&[
        "underwrap-secretsd".into(),
        "--bus-address".into(),
        "unix:path=/tmp/bus".into(),
        "--init-fd".into(),
        "nope".into(),
        "--publication-fd".into(),
        "8".into(),
    ])
    .unwrap_err();
    assert!(err.to_string().contains("--init-fd must be a number"));
    let err = parse_args_from(&["underwrap-secretsd".into(), "--bogus".into()]).unwrap_err();
    assert!(err.to_string().contains("unknown argument"));
}
