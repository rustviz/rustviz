fn chain(cond: Option<bool>) {
    let shared = 1;
    let mut exclusive = 2;
    let result = if let Some(ok) = cond && ok {
        Some((&shared, &mut exclusive))
    } else {
        None
    };
    println!("{shared} {result:?}");
}
