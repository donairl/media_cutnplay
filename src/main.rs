fn main() -> iced::Result {
    ffmpeg_next::init().expect("Failed to initialize FFmpeg");
    iced::application(|| (), update, view)
        .title("Media CutNPlay")
        .run()
}

#[derive(Debug, Clone)]
enum Message {}

fn update(_state: &mut (), _message: Message) {}

fn view(_state: &()) -> iced::Element<'_, Message> {
    iced::widget::text("Media CutNPlay Initialized").into()
}
