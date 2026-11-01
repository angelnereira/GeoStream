import 'dart:js_interop';

extension type _Window._(JSObject _) implements JSObject {
  external JSAny? open(String url, String target);
}

@JS('window')
external _Window get _window;

/// Abre una dirección en la misma pestaña.
void openLink(String url) {
  _window.open(url, '_self');
}
