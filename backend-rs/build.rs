fn generate_nyc_tile_set() {
    let src = "static_resources/nyc_tiles.json";
    println!("cargo:rerun-if-changed={}", src);

    let json_str = std::fs::read_to_string(src).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

    let mut packed: Vec<u64> = entries
        .iter()
        .map(|v| v["enc"].as_u64().unwrap())
        .collect();

    packed.sort_unstable();
    packed.dedup();

    let entries: String = packed.iter().map(|v| format!("{v}_u64,")).collect::<Vec<_>>().join("");
    let content = format!(
        "static NYC_TILE_SET: [u64; {}] = [{entries}];\n",
        packed.len()
    );

    let mut out_file = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    out_file.push("nyc_tile_set.rs");
    std::fs::write(out_file, content).unwrap();
}

fn generate_meshdb_client() {
    let src = "static_resources/meshdb_openapi.json";
    println!("cargo:rerun-if-changed={}", src);

    let file = std::fs::File::open(src).unwrap();
    let spec = serde_json::from_reader(file).unwrap();
    let mut generator = progenitor::Generator::default();
    let tokens = generator.generate_tokens(&spec).unwrap();
    let ast = syn::parse2(tokens).unwrap();
    let content = prettyplease::unparse(&ast);

    let mut out_file = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    out_file.push("meshdb_client.rs");
    std::fs::write(out_file, content).unwrap();
}


fn main() {
    generate_nyc_tile_set();
    generate_meshdb_client();
}
