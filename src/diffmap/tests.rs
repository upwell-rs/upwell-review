use super::*;

const QUOTED_DIFF: &str = concat!(
    "diff --git \"a/caf\\303\\251 \\\"q\\\"\\tx.rs\" \"b/caf\\303\\251 \\\"q\\\"\\tx.rs\"\n",
    "new file mode 100644\n",
    "--- /dev/null\n",
    "+++ \"b/caf\\303\\251 \\\"q\\\"\\tx.rs\"\t\n",
    "@@ -0,0 +1,2 @@\n",
    "+a\n",
    "+b\n",
    "diff --git a/back\\\\slash.rs b/back\\\\slash.rs\n",
    "--- \"a/back\\\\slash.rs\"\n",
    "+++ \"b/back\\\\slash.rs\"\n",
    "@@ -3,1 +3,1 @@\n",
    "-old\n",
    "+new\n",
);

fn lines(numbers: &[u32]) -> BTreeSet<u32> {
    numbers.iter().copied().collect()
}

#[test]
fn quoted_paths_are_decoded() {
    let files = diff_lines(QUOTED_DIFF);
    let expected = LineMap::from([
        ("café \"q\"\tx.rs".to_owned(), lines(&[1, 2])),
        ("back\\slash.rs".to_owned(), lines(&[3])),
    ]);

    assert_eq!(files, expected);
}

#[test]
fn added_lines_with_unicode_line_separators_do_not_shift_line_numbers() {
    let diff = "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -0,0 +1,3 @@\n+a\u{2028}b\n+c\x0cd\n+e\n";

    assert_eq!(
        diff_lines(diff),
        LineMap::from([("x.rs".to_owned(), lines(&[1, 2, 3]))])
    );
}

#[test]
fn context_lines_count_and_removed_lines_do_not() {
    let patch = "@@ -10,4 +20,4 @@ fn f() {\n context\n-removed\n+added\n context\n\\ No newline at end of file\n";

    assert_eq!(patch_lines(patch), lines(&[20, 21, 22]));
}

#[test]
fn several_hunks_restart_the_line_count() {
    let patch = "@@ -1 +1 @@\n-a\n+b\n@@ -50,2 +60,2 @@\n x\n+y\n";

    assert_eq!(patch_lines(patch), lines(&[1, 60, 61]));
}

#[test]
fn lines_before_the_first_hunk_and_malformed_hunks_are_ignored() {
    let patch = "+not in a hunk\n@@ -x +1 @@\n+still not\n";

    assert!(patch_lines(patch).is_empty());
}

#[test]
fn a_deleted_file_has_no_commentable_lines() {
    let diff = "diff --git a/gone.rs b/gone.rs\ndeleted file mode 100644\n--- a/gone.rs\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-a\n-b\n";

    assert!(diff_lines(diff).is_empty());
}

#[test]
fn every_file_of_a_diff_is_mapped() {
    let diff = concat!(
        "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,2 @@\n a\n+b\n",
        "diff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -5 +5 @@\n-x\n+y\n",
    );
    let files = diff_lines(diff);

    assert!(is_visible(&files, "a.rs", 2));
    assert!(is_visible(&files, "b.rs", 5));
    assert!(!is_visible(&files, "b.rs", 6));
    assert!(!is_visible(&files, "c.rs", 1));
}

#[test]
fn unquote_keeps_unknown_escapes_and_out_of_range_octals() {
    assert_eq!(unquote("a\\qb"), "a\\qb");
    assert_eq!(unquote("\\777"), "\\777");
    assert_eq!(unquote("\\101\\n"), "A\n");
}
