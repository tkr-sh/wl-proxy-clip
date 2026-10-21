pub(crate) const TEXT_MIMES: &[&str] = &[
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "TEXT",
    "STRING",
];

pub(crate) fn text_mime_for(mimes: &[String]) -> Option<&'static str> {
    TEXT_MIMES.iter().copied().find(|candidate| {
        mimes
            .iter()
            .any(|mime| mime.eq_ignore_ascii_case(candidate))
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ImageMime {
    pub(crate) request:   &'static str,
    pub(crate) canonical: &'static str,
}

const IMAGE_MIMES: &[ImageMime] = &[
    ImageMime {
        request:   "image/png",
        canonical: "image/png",
    },
    ImageMime {
        request:   "image/jpeg",
        canonical: "image/jpeg",
    },
    ImageMime {
        request:   "image/jpg",
        canonical: "image/jpeg",
    },
];

pub(crate) fn image_mime_for(mimes: &[String]) -> Option<ImageMime> {
    IMAGE_MIMES.iter().copied().find(|candidate| {
        mimes
            .iter()
            .any(|mime| mime.eq_ignore_ascii_case(candidate.request))
    })
}

pub(crate) fn is_paste_mime(mime: &str) -> bool {
    TEXT_MIMES
        .iter()
        .any(|candidate| mime.eq_ignore_ascii_case(candidate)) ||
        IMAGE_MIMES
            .iter()
            .any(|candidate| mime.eq_ignore_ascii_case(candidate.request))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_png_over_jpeg() {
        let mimes = vec!["image/jpeg".to_owned(), "image/png".to_owned()];

        assert_eq!(image_mime_for(&mimes), IMAGE_MIMES.first().copied());
    }

    #[test]
    fn canonicalizes_jpg_to_jpeg() {
        let mimes = vec!["image/jpg".to_owned()];

        assert_eq!(image_mime_for(&mimes), IMAGE_MIMES.get(2).copied());
    }

    #[test]
    fn falls_back_to_text() {
        let mimes = vec!["text/plain".to_owned()];

        assert_eq!(image_mime_for(&mimes), None);
        assert_eq!(text_mime_for(&mimes), Some("text/plain"));
    }

    #[test]
    fn recognizes_pasteable_mimes() {
        assert!(is_paste_mime("text/plain"));
        assert!(is_paste_mime("image/png"));
        assert!(is_paste_mime("IMAGE/JPEG"));
        assert!(is_paste_mime("image/jpg"));
        assert!(!is_paste_mime("image/webp"));
        assert!(!is_paste_mime("text/html"));
    }
}
