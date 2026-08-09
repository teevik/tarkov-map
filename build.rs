fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/tarkov-map-icon.ico");
        res.compile().unwrap();
    }
}
