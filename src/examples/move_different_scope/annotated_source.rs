fn main() {
    let <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="4">String::from</tspan>("hello");
    let <tspan data-hash="3">z</tspan> = {
        let <tspan data-hash="2">y</tspan> = <tspan data-hash="1">x</tspan>;
        <tspan class="fn" data-hash="0" hash="5">println!</tspan>("{}", <tspan data-hash="2">y</tspan>);
        // ...
    };
    <tspan class="fn" data-hash="0" hash="5">println!</tspan>("Hello, world!");
}