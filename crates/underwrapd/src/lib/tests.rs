use super::*;

#[test]
fn public_modules_are_accessible() {
    let _ = audit::read_granted as fn(&str, &str, Option<u32>);
    let _ = server::run;
}
