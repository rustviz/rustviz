pub fn f(assign: bool, consume: bool, refill: bool) {
    let mut s: String;
    if assign {
        s = String::new();
        if consume {
            drop(s);
        }
    } else {
        let _unassigned = 0;
        let _also_unassigned = 0;
    }
    while refill {
        s = String::new();
        break;
    }
    let _after = 0;
}
