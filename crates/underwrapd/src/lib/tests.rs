use super::*;

#[test]
fn public_modules_are_accessible() {
    let _ = audit::read_allowed as fn(&str, &str, Option<u32>);
    let _ = audit::read_denied as fn(&str, &str, Option<u32>);
    let _ = audit::auth_denied as fn(&str, &str, Option<u32>, Option<u32>);
    let _ = audit::resource_checkout as fn(&str, &str);
    let _ = audit::resource_checkin as fn(&str, &str);
    let _ = audit::resource_usage_signal as fn(&str, &str);
    let _ = audit::resource_checkin_denied as fn(&str, &str, &str);
    let _ = server::run;
}
