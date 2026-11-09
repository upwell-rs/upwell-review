use super::*;

fn shell(script: &str) -> Command {
    let mut command = Tools { mise: false }.command("sh");

    command.args(["-c", script]);
    command
}

#[test]
fn stdout_is_returned_and_stdin_is_fed() {
    let output = Tools::run(&mut shell("cat"), Some(b"payload")).expect("cat succeeds");

    assert_eq!(output, "payload");
}

#[test]
fn a_failure_reports_the_command_and_its_stderr() {
    let error = Tools::run(&mut shell("echo oops >&2; exit 4"), None).expect_err("exit 4 fails");

    assert_eq!(
        error.to_string(),
        "sh -c echo oops >&2; exit 4 failed: oops"
    );
}

#[test]
fn json_output_is_decoded() {
    let value: Vec<u32> = Tools::json(&mut shell("echo '[1, 2]'"), None).expect("valid json");

    assert_eq!(value, [1, 2]);
    assert!(matches!(
        Tools::json::<Vec<u32>>(&mut shell("echo nope"), None),
        Err(ToolError::Output { .. })
    ));
}

#[test]
fn mise_wraps_the_program_but_not_the_messages() {
    let mut command = Tools { mise: true }.command("gh");

    command.args(["api", "-X", "POST", "repos/x/pulls/1/reviews"]);

    assert_eq!(command.get_program(), "mise");
    assert_eq!(command_line(&command), "gh api -X POST");
}

#[test]
fn programs_are_found_on_path() {
    assert!(find_in_path("sh").is_some());
    assert!(find_in_path("upwell-review-test-missing-cli").is_none());
}
