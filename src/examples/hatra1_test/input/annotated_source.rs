fn main() {
    let <tspan data-hash="1">s</tspan> = <tspan class="fn" data-hash="0">String::from</tspan>("hello");
    <tspan class="fn" data-hash="0">takes_ownership</tspan>(<tspan data-hash="1">s</tspan>);
    let mut <tspan data-hash="2">x</tspan> = 5;
    let <tspan data-hash="3">y</tspan> = <tspan data-hash="2">x</tspan>;
    <tspan data-hash="2">x</tspan> = 6;
}

fn <tspan class="fn" data-hash="0">takes_ownership</tspan>(<tspan data-hash="4">some_string</tspan>: String) {
    <tspan class="fn" data-hash="0">println!</tspan>("{}", <tspan data-hash="4">some_string</tspan>);
}