// A reference read in a let-chain's later part (`x.is_empty()`) whose
// earlier `let` part borrows the same lender (`v.first()`): the read was
// drawn two rows below x's borrow end, on a stretch of x's column that
// had already ended (#1424). The read now shares the end's row, before
// it, as on any line whose stacked arrows end a borrow.
fn let_chain_after_first() {
    let v1 = vec![String::from("rust")];
    let x1 = &v1[0];
    if let Some(_y1) = v1.first() && x1.is_empty() {
    }
}

fn let_chain_after_let_else() {
    let v2 = vec![String::from("rust")];
    let Some(x2) = v2.first() else { return; };
    if let Some(_y2) = v2.first() && x2.is_empty() {
    }
}

fn while_let_chain() {
    let v3 = vec![String::from("rust")];
    let x3 = &v3[0];
    while let Some(_y3) = v3.first() && x3.is_empty() {
        break;
    }
}

fn three_part_chain() {
    let v4 = vec![String::from("rust")];
    let x4 = &v4[0];
    let c = true;
    if c && let Some(_y4) = v4.first() && x4.is_empty() {
    }
}

fn chain_in_else_if() {
    let v5 = vec![String::from("rust")];
    let x5 = &v5[0];
    let n = 3;
    if n > 5 {
        println!("big");
    } else if let Some(_y5) = v5.first() && x5.is_empty() {
        println!("empty");
    }
}

fn chain_with_ref_param(v6: &Vec<String>, x6: &String) {
    if let Some(_y6) = v6.first() && x6.is_empty() {
    }
}

fn chain_ref_used_after() {
    let v7 = vec![String::from("rust")];
    let x7 = &v7[0];
    if let Some(_y7) = v7.first() && x7.is_empty() {
    }
    println!("{}", x7);
}
