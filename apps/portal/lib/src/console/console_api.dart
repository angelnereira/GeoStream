import 'dart:convert';

import 'package:http/http.dart' as http;

/// Fallo de una llamada a la consola, con el código del contrato (`docs/09` §6) si lo hubo.
class ConsoleApiException implements Exception {
  /// Crea la excepción.
  const ConsoleApiException(this.status, this.code, [this.field]);

  /// Código HTTP, o 0 si no hubo respuesta.
  final int status;

  /// Código de error del contrato (`UNAUTHENTICATED`, `VALIDATION_FAILED`…); vacío si no hubo cuerpo válido.
  final String code;

  /// Primer campo inválido de un `VALIDATION_FAILED`.
  final String? field;

  /// Sesión caducada o revocada: hay que volver a entrar.
  bool get isUnauthenticated => status == 401;

  @override
  String toString() => 'ConsoleApiException($status, $code)';
}

/// Sesión abierta tras canjear el código.
class ConsoleSessionInfo {
  /// Crea la sesión.
  const ConsoleSessionInfo({
    required this.token,
    required this.email,
    required this.created,
  });

  /// Token `gcs_…`: solo vive en memoria.
  final String token;

  /// Correo de la cuenta.
  final String email;

  /// Si esta verificación creó la cuenta.
  final bool created;
}

/// Organización de la cuenta.
class Organization {
  /// Crea la organización.
  const Organization({
    required this.id,
    required this.name,
    required this.role,
  });

  /// Lee la respuesta del contrato.
  factory Organization.fromJson(Map<String, Object?> json) => Organization(
    id: json['id']! as String,
    name: json['name']! as String,
    role: json['role']! as String,
  );

  /// Identificador `ten_…`.
  final String id;

  /// Nombre.
  final String name;

  /// `OWNER`, `ADMIN` o `VIEWER`.
  final String role;

  /// Si el rol permite crear proyectos y keys.
  bool get canManage => role == 'OWNER' || role == 'ADMIN';
}

/// Proyecto (aplicación) de una organización.
class Project {
  /// Crea el proyecto.
  const Project({required this.id, required this.name, required this.status});

  /// Lee la respuesta del contrato.
  factory Project.fromJson(Map<String, Object?> json) => Project(
    id: json['id']! as String,
    name: json['name']! as String,
    status: json['status']! as String,
  );

  /// Identificador `app_…`.
  final String id;

  /// Nombre.
  final String name;

  /// `ACTIVE` o `DISABLED`.
  final String status;
}

/// API key sin secreto.
class ApiKeyInfo {
  /// Crea la key.
  const ApiKeyInfo({
    required this.id,
    required this.label,
    required this.prefix,
    required this.scopes,
    required this.revoked,
  });

  /// Lee la respuesta del contrato.
  factory ApiKeyInfo.fromJson(Map<String, Object?> json) => ApiKeyInfo(
    id: json['id']! as String,
    label: json['label']! as String,
    prefix: json['prefix']! as String,
    scopes: [for (final s in json['scopes']! as List<Object?>) s! as String],
    revoked: json['revoked_at'] != null,
  );

  /// Identificador `key_…`.
  final String id;

  /// Etiqueta.
  final String label;

  /// Primeros caracteres, para reconocerla.
  final String prefix;

  /// Scopes concedidos.
  final List<String> scopes;

  /// Si ya está revocada.
  final bool revoked;
}

/// API key recién creada: el secreto solo existe aquí.
class CreatedApiKey {
  /// Crea el resultado.
  const CreatedApiKey({required this.key, required this.secret});

  /// Datos de la key.
  final ApiKeyInfo key;

  /// La key en claro; no se vuelve a mostrar.
  final String secret;
}

/// Cliente de `/v1/console` a través del proxy del portal (mismo origen, sin CORS).
///
/// El token de sesión vive solo en esta instancia: no se escribe en el almacenamiento del navegador,
/// en cookies ni en logs.
class ConsoleApi {
  /// Crea el cliente. [base] termina en `/` y apunta al proxy (`https://portal/api/`).
  ConsoleApi({
    required this.client,
    required this.base,
    this.timeout = const Duration(seconds: 10),
  });

  /// Cliente HTTP.
  final http.Client client;

  /// Base del proxy.
  final Uri base;

  /// Tiempo máximo por petición.
  final Duration timeout;

  String? _token;

  /// Si hay una sesión abierta.
  bool get signedIn => _token != null;

  /// Pide un código por correo. Responde igual exista o no la cuenta.
  Future<void> requestCode(String email) async {
    await _send('POST', 'v1/console/auth/code', body: {'email': email});
  }

  /// Canjea el código y abre la sesión.
  Future<ConsoleSessionInfo> verify(
    String email,
    String code, {
    required bool acceptTerms,
  }) async {
    final json = await _send(
      'POST',
      'v1/console/auth/verify',
      body: {'email': email, 'code': code, 'accept_terms': acceptTerms},
    );
    final token = json['token'];
    final account = json['account'];
    if (token is! String || account is! Map<String, Object?>) {
      throw const ConsoleApiException(0, '');
    }
    _token = token;
    return ConsoleSessionInfo(
      token: token,
      email: account['email']! as String,
      created: json['created'] == true,
    );
  }

  /// Cierra la sesión en el servidor y la olvida aquí.
  Future<void> logout() async {
    try {
      await _send('POST', 'v1/console/auth/logout', auth: true);
    } finally {
      _token = null;
    }
  }

  /// Organizaciones de la cuenta.
  Future<List<Organization>> organizations() async {
    final json = await _send('GET', 'v1/console/organizations', auth: true);
    return [for (final o in _list(json)) Organization.fromJson(o)];
  }

  /// Crea una organización.
  Future<Organization> createOrganization(String name) async {
    final json = await _send(
      'POST',
      'v1/console/organizations',
      auth: true,
      body: {'name': name},
    );
    return Organization.fromJson(json);
  }

  /// Proyectos de una organización.
  Future<List<Project>> projects(String organizationId) async {
    final json = await _send(
      'GET',
      'v1/console/organizations/$organizationId/projects',
      auth: true,
    );
    return [for (final p in _list(json)) Project.fromJson(p)];
  }

  /// Crea un proyecto.
  Future<Project> createProject(String organizationId, String name) async {
    final json = await _send(
      'POST',
      'v1/console/organizations/$organizationId/projects',
      auth: true,
      body: {'name': name},
    );
    return Project.fromJson(json);
  }

  /// API keys de un proyecto, sin secretos.
  Future<List<ApiKeyInfo>> keys(String organizationId, String projectId) async {
    final json = await _send(
      'GET',
      'v1/console/organizations/$organizationId/projects/$projectId/keys',
      auth: true,
    );
    return [for (final k in _list(json)) ApiKeyInfo.fromJson(k)];
  }

  /// Crea una API key; el secreto viene solo en este resultado.
  Future<CreatedApiKey> createKey(
    String organizationId,
    String projectId, {
    required String label,
    required List<String> scopes,
  }) async {
    final json = await _send(
      'POST',
      'v1/console/organizations/$organizationId/projects/$projectId/keys',
      auth: true,
      body: {'label': label, 'scopes': scopes},
    );
    final key = json['key'];
    final secret = json['secret'];
    if (key is! Map<String, Object?> || secret is! String) {
      throw const ConsoleApiException(0, '');
    }
    return CreatedApiKey(key: ApiKeyInfo.fromJson(key), secret: secret);
  }

  /// Revoca una API key.
  Future<void> revokeKey(
    String organizationId,
    String projectId,
    String keyId,
  ) async {
    await _send(
      'DELETE',
      'v1/console/organizations/$organizationId/projects/$projectId/keys/$keyId',
      auth: true,
    );
  }

  List<Map<String, Object?>> _list(Map<String, Object?> json) {
    final data = json['data'];
    if (data is! List<Object?>) throw const ConsoleApiException(0, '');
    return [for (final item in data) item! as Map<String, Object?>];
  }

  Future<Map<String, Object?>> _send(
    String method,
    String path, {
    bool auth = false,
    Map<String, Object?>? body,
  }) async {
    final request = http.Request(method, base.resolve(path));
    if (auth) {
      final token = _token;
      if (token == null) {
        throw const ConsoleApiException(401, 'UNAUTHENTICATED');
      }
      request.headers['Authorization'] = 'Bearer $token';
    }
    if (body != null) {
      request.headers['Content-Type'] = 'application/json';
      request.body = jsonEncode(body);
    }
    final http.Response response;
    try {
      response = await http.Response.fromStream(
        await client.send(request).timeout(timeout),
      ).timeout(timeout);
    } on Object {
      throw const ConsoleApiException(0, '');
    }
    Map<String, Object?> json = const {};
    if (response.body.isNotEmpty) {
      try {
        final decoded = jsonDecode(response.body);
        if (decoded is Map<String, Object?>) json = decoded;
      } on FormatException {
        // Un cuerpo que no es JSON se trata como una respuesta sin información.
      }
    }
    if (response.statusCode >= 200 && response.statusCode < 300) {
      return json;
    }
    if (response.statusCode == 401) _token = null;
    final errors = json['errors'];
    String? field;
    if (errors is List<Object?> && errors.isNotEmpty) {
      final first = errors.first;
      if (first is Map<String, Object?>) field = first['field'] as String?;
    }
    throw ConsoleApiException(
      response.statusCode,
      json['code'] is String ? json['code']! as String : '',
      field,
    );
  }
}
