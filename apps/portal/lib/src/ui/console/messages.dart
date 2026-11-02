import '../../console/console_api.dart';

/// El texto que ve la persona para un error de la API de consola. Nunca muestra detalle del servidor.
String messageFor(ConsoleApiException error) {
  // Un 404 sin cuerpo del contrato no viene de la API de la consola: la consola no está activada en este
  // entorno (sin clave de correo) y el borde no encuentra la ruta.
  if (error.status == 404 && error.code.isEmpty) {
    return 'La consola todavía no está activa en este entorno. Inténtalo más tarde.';
  }
  if (error.status == 0) {
    return 'No pudimos conectar con el servicio. Revisa tu conexión e inténtalo de nuevo.';
  }
  switch (error.code) {
    case 'UNAUTHENTICATED':
      return 'El código no es válido o ya caducó. Pide uno nuevo.';
    case 'RATE_LIMITED':
      return 'Demasiados intentos. Espera un momento y vuelve a intentarlo.';
    case 'VALIDATION_FAILED':
      switch (error.field) {
        case 'email':
          return 'Escribe un correo válido.';
        case 'code':
          return 'El código son solo números.';
        case 'password':
          return 'La contraseña debe tener entre 10 y 128 caracteres y no ser '
              'demasiado fácil de adivinar. Una frase larga funciona bien.';
        case 'accept_terms':
          return 'Para crear la cuenta tienes que aceptar las condiciones.';
        case 'name':
          return 'El nombre no es válido (1 a 80 caracteres).';
        case 'label':
          return 'La etiqueta no es válida (1 a 64 caracteres).';
        case 'scopes':
          return 'Elige al menos un permiso.';
        case 'allowed_origins':
          return 'Los orígenes deben ser como https://app.ejemplo.com, sin ruta, y no repetirse.';
        case 'project':
          return 'El proyecto está deshabilitado: actívalo para crear claves.';
      }
      return 'Revisa los datos e inténtalo de nuevo.';
    case 'PERMISSION_DENIED':
      return 'No se pudo: la contraseña actual no es correcta o tu rol no permite esta acción.';
    case 'CONFLICT':
      return 'No se puede hacer todavía: eres el único propietario de una '
          'organización con proyectos activos. Deshabilítalos o transfiérelos antes.';
    case 'NOT_FOUND':
      return 'No se encontró. Puede que ya no exista.';
  }
  return 'No pudimos completar la acción. Inténtalo de nuevo en un momento.';
}
