fn main() {
    let <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="3">String::from</tspan>("hello");
    let <tspan data-hash="2">y</tspan> = <tspan data-hash="1">x</tspan>;
    <tspan class="fn" data-hash="0" hash="4">println!</tspan>("{}", <tspan data-hash="1">x</tspan>) // ERROR: x does not own a resource
}