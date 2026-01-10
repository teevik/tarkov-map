fn main() {
    cynic_codegen::register_schema("tarkov")
        .from_sdl_file("schema.graphql")
        .unwrap()
        .as_default()
        .unwrap();

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/tarkov-map-icon.ico");
        res.compile().unwrap();
    }
}
