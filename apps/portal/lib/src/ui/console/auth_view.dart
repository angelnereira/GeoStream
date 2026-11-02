import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../console/console_api.dart';
import '../../platform/link_opener.dart';
import 'messages.dart';

/// Acceso a la consola: iniciar sesión (correo y contraseña, y después un código) o crear la cuenta
/// (correo, código y contraseña). Recuperar el acceso es repetir el registro (ADR-0037).
class AuthView extends StatefulWidget {
  /// Crea la pantalla.
  const AuthView({super.key, required this.api, required this.onSignedIn});

  /// Cliente de la consola.
  final ConsoleApi api;

  /// Se llama con la sesión abierta.
  final ValueChanged<ConsoleSessionInfo> onSignedIn;

  @override
  State<AuthView> createState() => _AuthViewState();
}

class _AuthViewState extends State<AuthView>
    with SingleTickerProviderStateMixin {
  late final TabController _tabs = TabController(length: 2, vsync: this);

  /// Si la pestaña «Crear cuenta» se abrió para recuperar el acceso.
  bool _recovering = false;

  @override
  void dispose() {
    _tabs.dispose();
    super.dispose();
  }

  void _recover() {
    setState(() => _recovering = true);
    _tabs.animateTo(1);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 480),
        child: ListView(
          padding: const EdgeInsets.all(16),
          shrinkWrap: true,
          children: [
            Text('Consola de GeoStream', style: theme.textTheme.headlineSmall),
            const SizedBox(height: 8),
            const Text(
              'Administra tus proyectos y las API keys con las que tu aplicación '
              'consume GeoStream. Para entrar necesitas tu contraseña y un código '
              'que enviamos a tu correo.',
            ),
            const SizedBox(height: 12),
            TabBar(
              controller: _tabs,
              onTap: (_) => setState(() => _recovering = false),
              tabs: const [
                Tab(text: 'Entrar'),
                Tab(text: 'Crear cuenta'),
              ],
            ),
            const SizedBox(height: 16),
            AnimatedBuilder(
              animation: _tabs,
              builder: (context, _) => _tabs.index == 0
                  ? SignInForm(
                      api: widget.api,
                      onSignedIn: widget.onSignedIn,
                      onForgot: _recover,
                    )
                  : RegisterForm(
                      key: ValueKey(_recovering),
                      api: widget.api,
                      onSignedIn: widget.onSignedIn,
                      recovering: _recovering,
                    ),
            ),
            const SizedBox(height: 8),
            Align(
              alignment: Alignment.centerLeft,
              child: TextButton.icon(
                onPressed: () => openLink('/docs/15_INTEGRATION_GUIDE.html'),
                icon: const Icon(Icons.menu_book_outlined, size: 18),
                label: const Text('Cómo se integra GeoStream (guía)'),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Campo de contraseña con botón para mostrarla u ocultarla.
class PasswordField extends StatefulWidget {
  /// Crea el campo.
  const PasswordField({
    super.key,
    required this.controller,
    required this.label,
    this.onSubmitted,
    this.newPassword = false,
    this.autofocus = false,
  });

  /// Controlador del texto.
  final TextEditingController controller;

  /// Etiqueta visible.
  final String label;

  /// Se llama al pulsar «intro».
  final VoidCallback? onSubmitted;

  /// Si es una contraseña nueva (para que el navegador ofrezca guardarla).
  final bool newPassword;

  /// Si toma el foco al aparecer.
  final bool autofocus;

  @override
  State<PasswordField> createState() => _PasswordFieldState();
}

class _PasswordFieldState extends State<PasswordField> {
  bool _visible = false;

  @override
  Widget build(BuildContext context) {
    return TextField(
      controller: widget.controller,
      obscureText: !_visible,
      autofocus: widget.autofocus,
      enableSuggestions: false,
      autocorrect: false,
      autofillHints: [
        widget.newPassword ? AutofillHints.newPassword : AutofillHints.password,
      ],
      decoration: InputDecoration(
        labelText: widget.label,
        border: const OutlineInputBorder(),
        suffixIcon: IconButton(
          tooltip: _visible ? 'Ocultar contraseña' : 'Mostrar contraseña',
          icon: Icon(_visible ? Icons.visibility_off : Icons.visibility),
          onPressed: () => setState(() => _visible = !_visible),
        ),
      ),
      onSubmitted: (_) => widget.onSubmitted?.call(),
    );
  }
}

mixin _Guarded<T extends StatefulWidget> on State<T> {
  bool busy = false;
  String? error;

  Future<void> guard(Future<void> Function() action) async {
    setState(() {
      busy = true;
      error = null;
    });
    try {
      await action();
    } on ConsoleApiException catch (e) {
      if (mounted) setState(() => error = messageFor(e));
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Widget errorText(BuildContext context) => error == null
      ? const SizedBox.shrink()
      : Padding(
          padding: const EdgeInsets.only(top: 12),
          child: Text(
            error!,
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        );
}

/// Iniciar sesión: correo y contraseña, y después el código que llega al correo.
class SignInForm extends StatefulWidget {
  /// Crea el formulario.
  const SignInForm({
    super.key,
    required this.api,
    required this.onSignedIn,
    required this.onForgot,
  });

  /// Cliente de la consola.
  final ConsoleApi api;

  /// Se llama con la sesión abierta.
  final ValueChanged<ConsoleSessionInfo> onSignedIn;

  /// La persona no recuerda su contraseña.
  final VoidCallback onForgot;

  @override
  State<SignInForm> createState() => _SignInFormState();
}

class _SignInFormState extends State<SignInForm> with _Guarded<SignInForm> {
  final TextEditingController _email = TextEditingController();
  final TextEditingController _password = TextEditingController();
  final TextEditingController _code = TextEditingController();
  bool _codeStep = false;

  @override
  void dispose() {
    _email.dispose();
    _password.dispose();
    _code.dispose();
    super.dispose();
  }

  Future<void> _credentials() => guard(() async {
    await widget.api.login(_email.text.trim(), _password.text);
    // La contraseña ya cumplió su papel: no se conserva en el formulario.
    _password.clear();
    if (mounted) setState(() => _codeStep = true);
  });

  Future<void> _verify() => guard(() async {
    final info = await widget.api.verifyLogin(
      _email.text.trim(),
      _code.text.trim(),
    );
    widget.onSignedIn(info);
  });

  @override
  Widget build(BuildContext context) {
    if (_codeStep) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Nodo propio: sin él, los lectores de pantalla leen el párrafo como parte de la etiqueta del campo.
          Semantics(
            container: true,
            child: Text(
              'Si el correo y la contraseña son correctos, te enviamos un código. '
              'Escríbelo para entrar. Caduca en 10 minutos; revisa también el spam.',
            ),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _code,
            autofocus: true,
            keyboardType: TextInputType.number,
            inputFormatters: [FilteringTextInputFormatter.digitsOnly],
            autofillHints: const [AutofillHints.oneTimeCode],
            decoration: const InputDecoration(
              labelText: 'Código',
              border: OutlineInputBorder(),
            ),
            onSubmitted: (_) => _verify(),
          ),
          errorText(context),
          const SizedBox(height: 16),
          FilledButton(
            onPressed: busy ? null : _verify,
            child: Text(busy ? 'Un momento…' : 'Entrar'),
          ),
          TextButton(
            onPressed: busy
                ? null
                : () => setState(() {
                    _codeStep = false;
                    _code.clear();
                    error = null;
                  }),
            child: const Text('Volver'),
          ),
        ],
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        TextField(
          controller: _email,
          keyboardType: TextInputType.emailAddress,
          autofillHints: const [AutofillHints.email],
          decoration: const InputDecoration(
            labelText: 'Correo',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 12),
        PasswordField(
          controller: _password,
          label: 'Contraseña',
          onSubmitted: _credentials,
        ),
        errorText(context),
        const SizedBox(height: 16),
        FilledButton(
          onPressed: busy ? null : _credentials,
          child: Text(busy ? 'Un momento…' : 'Continuar'),
        ),
        TextButton(
          onPressed: busy ? null : widget.onForgot,
          child: const Text(
            '¿Olvidaste tu contraseña o es tu primera vez con contraseña?',
          ),
        ),
      ],
    );
  }
}

/// Crear la cuenta (o recuperar el acceso): correo, código y contraseña.
class RegisterForm extends StatefulWidget {
  /// Crea el formulario.
  const RegisterForm({
    super.key,
    required this.api,
    required this.onSignedIn,
    this.recovering = false,
  });

  /// Cliente de la consola.
  final ConsoleApi api;

  /// Se llama con la sesión abierta.
  final ValueChanged<ConsoleSessionInfo> onSignedIn;

  /// Si es para recuperar el acceso a una cuenta existente.
  final bool recovering;

  @override
  State<RegisterForm> createState() => _RegisterFormState();
}

class _RegisterFormState extends State<RegisterForm>
    with _Guarded<RegisterForm> {
  final TextEditingController _email = TextEditingController();
  final TextEditingController _code = TextEditingController();
  final TextEditingController _password = TextEditingController();
  final TextEditingController _confirm = TextEditingController();
  bool _codeSent = false;
  bool _terms = false;

  @override
  void dispose() {
    _email.dispose();
    _code.dispose();
    _password.dispose();
    _confirm.dispose();
    super.dispose();
  }

  Future<void> _sendCode() => guard(() async {
    await widget.api.requestCode(_email.text.trim());
    if (mounted) setState(() => _codeSent = true);
  });

  Future<void> _create() async {
    if (_password.text != _confirm.text) {
      setState(() => error = 'Las dos contraseñas no coinciden.');
      return;
    }
    await guard(() async {
      final info = await widget.api.verify(
        _email.text.trim(),
        _code.text.trim(),
        password: _password.text,
        acceptTerms: _terms,
      );
      widget.onSignedIn(info);
    });
  }

  @override
  Widget build(BuildContext context) {
    final recovering = widget.recovering;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (recovering) ...[
          const Text(
            'Para recuperar el acceso te enviamos un código al correo de tu '
            'cuenta y eliges una contraseña nueva. Las cuentas creadas antes de '
            'que existieran las contraseñas también entran así la primera vez.',
          ),
          const SizedBox(height: 12),
        ],
        TextField(
          controller: _email,
          enabled: !_codeSent,
          keyboardType: TextInputType.emailAddress,
          autofillHints: const [AutofillHints.email],
          decoration: const InputDecoration(
            labelText: 'Correo',
            border: OutlineInputBorder(),
          ),
          onSubmitted: (_) => _codeSent ? null : _sendCode(),
        ),
        if (_codeSent) ...[
          const SizedBox(height: 12),
          Semantics(
            container: true,
            child: Text(
              'Si el correo es válido, te enviamos un código que caduca en 10 '
              'minutos. Revisa también la carpeta de spam.',
            ),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _code,
            keyboardType: TextInputType.number,
            inputFormatters: [FilteringTextInputFormatter.digitsOnly],
            autofillHints: const [AutofillHints.oneTimeCode],
            decoration: const InputDecoration(
              labelText: 'Código',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 12),
          PasswordField(
            controller: _password,
            label: recovering ? 'Contraseña nueva' : 'Elige una contraseña',
            newPassword: true,
          ),
          const SizedBox(height: 4),
          Text(
            'Mínimo 10 caracteres. Una frase larga es mejor que símbolos raros.',
            style: Theme.of(context).textTheme.bodySmall,
          ),
          const SizedBox(height: 12),
          PasswordField(
            controller: _confirm,
            label: 'Repite la contraseña',
            newPassword: true,
            onSubmitted: _create,
          ),
          CheckboxListTile(
            contentPadding: EdgeInsets.zero,
            controlAffinity: ListTileControlAffinity.leading,
            value: _terms,
            onChanged: (v) => setState(() => _terms = v ?? false),
            title: const Text(
              'Acepto las condiciones de uso y el tratamiento de mi correo para '
              'crear la cuenta (solo si es tu primera vez).',
            ),
          ),
        ],
        errorText(context),
        const SizedBox(height: 16),
        FilledButton(
          onPressed: busy ? null : (_codeSent ? _create : _sendCode),
          child: Text(
            busy
                ? 'Un momento…'
                : _codeSent
                ? (recovering ? 'Cambiar contraseña y entrar' : 'Crear cuenta')
                : 'Enviarme un código',
          ),
        ),
        if (_codeSent)
          TextButton(
            onPressed: busy
                ? null
                : () => setState(() {
                    _codeSent = false;
                    _code.clear();
                    _password.clear();
                    _confirm.clear();
                    error = null;
                  }),
            child: const Text('Usar otro correo o pedir otro código'),
          ),
      ],
    );
  }
}
