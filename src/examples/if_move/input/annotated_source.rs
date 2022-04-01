fn main() {
    let <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="3">String::from</tspan>("ABC");
    let <tspan data-hash="2">guard</tspan> = 1;
    if <tspan data-hash="2">guard</tspan> == 1 {
        <tspan class="fn" data-hash="0" hash="4">takes_ownership</tspan>(<tspan data-hash="1">s</tspan>);
    }
}

fn <tspan class="fn" data-hash="0" hash="4">takes_ownership</tspan>(<tspan data-hash="2">some_string</tspan>: String) {
    <tspan class="fn" data-hash="0" hash="5">println!</tspan>("{}", <tspan data-hash="2">some_string</tspan>);
}