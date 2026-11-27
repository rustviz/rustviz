fn array_arm(cond: bool, pair: [String; 2], other: [String; 2]) {
    {
        let a = pair;
        if cond {
            drop(a.len());
            let [s, _] = a;
            drop(s);
        }
    }
    { let a = other; drop(a); }
}
