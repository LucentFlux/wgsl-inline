# WGSL Inline
[![crates.io](https://img.shields.io/crates/v/wgsl-inline.svg)](https://crates.io/crates/wgsl-inline)
[![docs.rs](https://img.shields.io/docsrs/wgsl-inline)](https://docs.rs/wgsl-inline/latest/wgsl_inline/)
[![crates.io](https://img.shields.io/crates/l/wgsl-inline.svg)](https://github.com/LucentFlux/wgsl-inline/blob/main/LICENSE)

WGSL Inline adds a macro, `wgsl!`, which takes WGSL sourcecode and validates it, reporting any errors to the Rust compiler. 

Note that this crate is intended for small shaders - if you have a large program, with many shaders, you may prefer the [include-wgsl-oil](https://crates.io/crates/include-wgsl-oil) crate, which allows shaders to import other shader files using the `naga-oil` preprocessor.

# Example

In your `Cargo.toml`:

```toml
wgsl-inline = "0.2"
```

Then in your Rust source:

```rust
mod my_shader {
    wgsl_inline::wgsl!{
        struct VertexOutput {
            @builtin(position) position: vec4<f32>,
            @location(0) frag_uv: vec2<f32>,
        }

        @vertex
        fn main(
            @location(0) position: vec4<f32>,
            @location(1) uv: vec2<f32>
        ) -> VertexOutput {
            var output: VertexOutput;
            output.position = position;
            output.frag_uv = uv;
            return output;
        }
    }
}

fn main() {
    // The generated `SOURCE` constant contains the source code,
    // with the added guarantee that the shader is valid.
    println!("shader source: {}", my_shader::SOURCE);
}
```

# Error Checking

Error scopes are propogated to the token in the macro that caused the error. That is to say, your IDE should be able to tell you exactly which bit of the shader code isn't valid, without ever leaving Rust! For example, my IDE shows me something like the following:

![Image of a WGSL compile error in an IDE](https://raw.githubusercontent.com/LucentFlux/wgsl-inline/main/docs/images/compile_error.png)

# Exported items

This crate uses `naga-to-tokenstream` to extract information other than just the shader source into your Rust program. For a full list of the generated items, 
see [naga-to-tokenstream](https://crates.io/crates/naga-to-tokenstream). The `encase`, `glam` and `naga` features of the `naga-to-tokenstream` crate can be enabled using feature flags of the same name on this crate.

# Minification

This crate comes with a "minification" feature flag `minify`. When enabled, all of your included shader source code will be reduced in size at compile time (removing variable names and excess whitespace). This is intended to be used on release builds, stripping debug information to increase shader parsing startup time and decrease read latency.

```toml
wgsl-inline = { version = "0.2", features = ["minify"] }
```
