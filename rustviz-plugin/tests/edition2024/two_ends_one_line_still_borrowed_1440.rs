// Two shared loans of one lender ending on one line, drawn on two rows:
// the end of a borrow made on the line goes on a row below the line's
// other arrows. The first end used to say the lender was "no longer
// immutably borrowed" while the other reference still borrowed it (#1440);
// it now names that reference. Each function is one context: a let-chain,
// the same in an arm, in an `else if`, and a block expression in a `let`.
// The last function is the same text in an arm: x2's end, copied to the
// start of the `else` arm where x2 isn't used, comes before r's end there.

fn let_chain() {
    let v1 = vec![String::from("rust")];
    let x1 = &v1[0];
    if let Some(_y1) = v1.first() && x1.is_empty() {
    }
}

fn let_chain_in_arm(c: bool) {
    let v2 = vec![String::from("rust")];
    let x2 = &v2[0];
    if c {
        if let Some(_y2) = v2.first() && x2.is_empty() {
        }
    }
}

fn let_chain_in_else_if(c: bool) {
    let v3 = vec![String::from("rust")];
    let x3 = &v3[0];
    if c {
        println!("{}", x3);
    } else if let Some(_y3) = v3.first() && x3.is_empty() {
    }
}

fn block_in_let() {
    let v4 = vec![String::from("rust")];
    let x4 = &v4[0];
    let n = { let y4 = &v4; y4.len() + x4.len() };
    println!("{}", n);
}

fn other_arm_end_while_shared(c: bool) {
    let s5 = String::from("x");
    let r5 = &s5;
    let x5 = &s5;
    if c {
        println!("{}", x5);
    } else {
        println!("{}", r5);
    }
}

fn main() {
    let_chain();
    let_chain_in_arm(true);
    let_chain_in_else_if(false);
    block_in_let();
    other_arm_end_while_shared(false);
}
