/// Fuente de mediciones real sobre `geolocator`.
library;

import 'package:geolocator/geolocator.dart';

import 'sample_mapper.dart';

/// Pide permiso «mientras se usa» y emite mediciones de máxima precisión.
///
/// Perfil `navigation` de `docs/12_FLUTTER_SDK.md` §11: filtro de 5 m.
Stream<RawFix> geolocatorFixes() async* {
  if (!await Geolocator.isLocationServiceEnabled()) {
    throw const LocationServiceDisabledException();
  }
  var permission = await Geolocator.checkPermission();
  if (permission == LocationPermission.denied) {
    permission = await Geolocator.requestPermission();
  }
  if (permission == LocationPermission.denied ||
      permission == LocationPermission.deniedForever) {
    throw const PermissionDeniedException('Permiso de ubicación denegado');
  }
  yield* Geolocator.getPositionStream(
    locationSettings: const LocationSettings(distanceFilter: 5),
  ).map(
    (p) => RawFix(
      latitude: p.latitude,
      longitude: p.longitude,
      accuracyM: p.accuracy,
      timestamp: p.timestamp,
      altitudeM: p.altitude,
      altitudeAccuracyM: p.altitudeAccuracy,
      speedMps: p.speed,
      speedAccuracyMps: p.speedAccuracy,
      headingDeg: p.heading,
      headingAccuracyDeg: p.headingAccuracy,
      isMock: p.isMocked,
    ),
  );
}
