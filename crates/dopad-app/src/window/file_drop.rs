use gpui_kit::{
    App, Bounds, DispatchPhase, Element, FileDropEvent, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Style, Window,
};

type FileDropListener = dyn Fn(&FileDropEvent, &mut Window, &mut App) + 'static;

pub(super) struct Capture {
    listener: Option<Box<FileDropListener>>,
}

impl Capture {
    pub(super) fn new(listener: impl Fn(&FileDropEvent, &mut Window, &mut App) + 'static) -> Self {
        Self {
            listener: Some(Box::new(listener)),
        }
    }
}

impl Element for Capture {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui_kit::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (window.request_layout(Style::default(), [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<gpui_kit::Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<gpui_kit::Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let Some(listener) = self.listener.take() else {
            return;
        };
        window.on_mouse_event(move |event, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                listener(event, window, cx);
            }
        });
    }
}

impl IntoElement for Capture {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
