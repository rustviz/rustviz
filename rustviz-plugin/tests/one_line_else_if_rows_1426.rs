// A one-line conditional whose later arm stacks arrows on rows of its
// own (`if n > 2 { } else if let Some(x) = v.first() { let r = x; }`):
// that arm's column started on the split row, above the fork's diagonal
// into it, so its first event sat on a stretch of the column that isn't
// drawn and its leg met the diagonal in mid-air. The fork now goes half a
// row up, as when the first arm starts on the split row. In the arm, the
// end of a reference given its borrow on the line (`r = x`) comes after
// that, below the stack, not on its first row; and the ends of references
// a conditional nested later on the line declares go into its arm (#1426).
fn one_line_else_if_let() {
    let v1 = vec![String::from("a")];
    let n = 1;
    if n > 2 { } else if let Some(x1) = v1.first() { let r1 = x1; }
}

fn one_line_else_if_let_else() {
    let v2 = vec![String::from("a")];
    let n = 1;
    if n > 2 { } else if let Some(x2) = v2.first() { let r2 = x2; } else { }
}

fn two_line_else_if_let() {
    let v3 = vec![String::from("a")];
    let n = 1;
    if n > 2 {
    } else if let Some(x3) = v3.first() { let r3 = x3; }
}

fn match_arm_one_line_if_let() {
    let v4 = vec![String::from("a")];
    let n = 1;
    match n { 2 => {} _ => if let Some(x4) = v4.first() { let r4 = x4; } }
}

fn nested_one_line_if_let_else() {
    let v5 = vec![String::from("a")];
    let n = 1;
    if n > 2 {
    } else {
        if let Some(x5) = v5.first() { let r5 = x5; } else { }
    }
}

fn one_line_else_stack() {
    let v6 = vec![String::from("a")];
    let n = 1;
    if n > 2 { } else { let x6 = &v6; let r6 = x6; }
}

fn one_line_else_if_cond_stack() {
    let v7 = vec![String::from("a")];
    let n = 1;
    if n > 2 { } else if n > 0 { let x7 = &v7; let r7 = x7; }
}
