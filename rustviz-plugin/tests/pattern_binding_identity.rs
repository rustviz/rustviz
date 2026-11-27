fn inspect(a: usize, short: ([String; 2],), long: ([String; 3],)) {
    {
        let (a,) = short;
        let [first, _] = a;
        drop(first);
    }
    {
        let (a,) = long;
        let [_, middle, _] = a;
        drop(middle);
    }
    let _ = a;
}
