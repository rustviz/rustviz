fn match_arm(mut s: String, arm: bool, write: bool) {
    let old = s;
    if true {
    } else {
        match arm {
            true => {
                if write { s = String::from("replacement"); }
            }
            false => {}
        }
    }
    drop(old);
}

fn let_else(mut s: String, write: bool) {
    let old = s;
    if true {
    } else {
        let Some(()) = Some(()) else {
            if write { s = String::from("replacement"); }
            return;
        };
    }
    drop(old);
}

fn loop_break(mut s: String, write: bool) {
    let old = s;
    loop {
        if true { break; }
        if write { s = String::from("replacement"); }
        break;
    }
    drop(old);
}

fn question_mark(mut s: String, write: bool, tail: Option<()>) -> Option<()> {
    let old = s;
    if true {
    } else {
        if write { s = String::from("replacement"); }
        tail?;
    }
    drop(old);
    Some(())
}

fn both_assign(mut s: String, write: bool) {
    let old = s;
    if true {
    } else {
        if write {
            s = String::from("left");
        } else {
            s = String::from("right");
        }
        drop(s);
    }
    drop(old);
}

fn never_assigned(write: bool) {
    let s: String;
    if true {
    } else {
        if write { s = String::new(); }
    }
}

fn closure_never_returns(owner: String) -> Option<()> {
    let stop = || -> Option<()> { panic!("stop") };
    stop()?;
    drop(owner);
    Some(())
}
