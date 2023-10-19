mod shader {
    wgsl_inline::wgsl! {
        struct MyStruct {
            foo: f32,
            bar: u32
        }
    }
}

fn main() {
    let my_struct = shader::types::MyStruct { foo: 1.0, bar: 12 };

    println!("my struct: {:?}", my_struct);
}
