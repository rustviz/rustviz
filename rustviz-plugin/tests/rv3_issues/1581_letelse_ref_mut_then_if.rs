pub fn f(mut opt: Option<String>, c: bool) {
    let Some(ref mut binding) = opt else {
        return;
}; if c { binding.push('a') } else { binding.push('b') }
    let _k = 0;
}
