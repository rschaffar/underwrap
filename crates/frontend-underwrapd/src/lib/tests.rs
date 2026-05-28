use super::*;

#[test]
fn public_modules_are_accessible() {
    let _ = audit::read_granted as fn(&str, &str, Option<u32>, Option<&str>);
    let _ = server::run;
}
