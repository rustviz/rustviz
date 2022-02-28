fn main() {
    let <tspan data-hash="1">s</tspan> = <tspan class="fn" data-hash="0" hash="3">String::from</tspan>("hello");
    <tspan class="fn" data-hash="0" hash="4">takes_ownership</tspan>(<tspan data-hash="1">s</tspan>);
    // println!("{}", s) // won't compile if added
}

fn <tspan class="fn" data-hash="0" hash="4">takes_ownership</tspan>(<tspan data-hash="2">some_string</tspan>: String) {
    <tspan class="fn" data-hash="0" hash="5">println!</tspan>("{}", <tspan data-hash="2">some_string</tspan>);
}