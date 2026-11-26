// Known bug, from the closed 2.x fix PRs (cx/hedge-f1); kept as a RustViz 3 regression case.
// Issue(s): #1682 (see known_bugs/README.md for what main draws wrong).
fn main() {
    let r = String::from("r");
    let s2 = String::from("ab");
    let v = vec![s2, format!("{}!", r)];
    println!("{}", v.len());
}

fn consume(_: Vec<String>) {}

fn assignment(mut dst: Vec<String>, item: String) {
    dst = vec![format!("first"), item];
    consume(dst);
}

fn argument(arg: String) {
    consume(vec![arg, format!("last")]);
}

fn branch(flag: bool, yes: String, no: String) -> Vec<String> {
    if flag {
        vec![yes, format!("yes")]
    } else {
        vec![format!("no"), no]
    }
}

fn loop_head(head: String) {
    for value in vec![format!("first"), head] {
        drop(value);
    }
}

fn nested_list(nested: String) {
    let lists = vec![vec![nested, format!("inner")]];
    drop(lists);
}

fn maybe_finished(first: String, next: Option<String>) -> Option<Vec<String>> {
    let values = vec![first, next?, format!("last")];
    Some(values)
}

#[allow(unreachable_code, unused_variables)]
fn unfinished(early: String, later: String) {
    let missing = vec![early, panic!("stop"), later];
}

#[allow(unreachable_code, unused_assignments)]
fn unfinished_assignment(mut previous: Vec<String>, part: String) {
    previous = vec![part, panic!("stop")];
    drop(previous);
}

fn nested_before_return(inner: String, tail: Option<Vec<String>>) -> Option<()> {
    let outer = vec![vec![inner, format!("inner")], tail?];
    drop(outer);
    Some(())
}
