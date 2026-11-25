/// Helpers for resolving and formatting human-readable Android & iOS hardware model names.

const Set<String> _legacyDefaultNames = {
  'veloce android',
  'veloce iphone',
  'veloce mobile',
  'veloce desktop',
  'veloce cli',
};

/// Returns true when [name] is blank or one of the old generic Veloce placeholder names.
bool isLegacyDefaultDeviceName(String? name) {
  final trimmed = name?.trim() ?? '';
  if (trimmed.isEmpty) return true;
  return _legacyDefaultNames.contains(trimmed.toLowerCase());
}

/// Formats an OEM manufacturer + hardware model pair without duplicating brand prefixes.
String formatManufacturerModel(String manufacturer, String model) {
  final rawVendor = manufacturer.trim();
  final rawModel = model.trim();
  if (rawModel.isEmpty && rawVendor.isEmpty) return '';

  const brandAliases = <String, String>{
    'samsung': 'Samsung',
    'google': 'Google',
    'xiaomi': 'Xiaomi',
    'redmi': 'Redmi',
    'poco': 'POCO',
    'oneplus': 'OnePlus',
    'oppo': 'OPPO',
    'vivo': 'vivo',
    'realme': 'realme',
    'motorola': 'Motorola',
    'nothing': 'Nothing',
    'sony': 'Sony',
    'asus': 'ASUS',
    'huawei': 'HUAWEI',
    'honor': 'HONOR',
    'lenovo': 'Lenovo',
    'apple': 'Apple',
  };

  final vendorLower = rawVendor.toLowerCase();
  final vendor = brandAliases[vendorLower] ??
      (rawVendor.isEmpty
          ? ''
          : '${rawVendor[0].toUpperCase()}${rawVendor.substring(1)}');

  if (rawModel.isEmpty) return vendor;
  if (vendor.isEmpty) return rawModel;

  final modelLower = rawModel.toLowerCase();
  if (modelLower.startsWith(vendorLower)) {
    return rawModel;
  }
  if (vendorLower == 'google' && modelLower.startsWith('pixel ')) {
    return rawModel;
  }
  return '$vendor $rawModel';
}

/// Maps Apple `utsname.machine` identifiers (e.g. `iPhone16,2`, `iPad14,5`) to marketing model names.
String mapAppleModelIdentifier(String machine) {
  final id = machine.trim();
  if (id.isEmpty) return '';

  const table = <String, String>{
    // iPhone 17 / Air series
    'iPhone18,1': 'iPhone 17 Pro',
    'iPhone18,2': 'iPhone 17 Pro Max',
    'iPhone18,3': 'iPhone 17',
    'iPhone18,4': 'iPhone Air',
    // iPhone 16 series
    'iPhone17,1': 'iPhone 16 Pro',
    'iPhone17,2': 'iPhone 16 Pro Max',
    'iPhone17,3': 'iPhone 16',
    'iPhone17,4': 'iPhone 16 Plus',
    'iPhone17,5': 'iPhone 16e',
    // iPhone 15 series
    'iPhone15,4': 'iPhone 15',
    'iPhone15,5': 'iPhone 15 Plus',
    'iPhone16,1': 'iPhone 15 Pro',
    'iPhone16,2': 'iPhone 15 Pro Max',
    // iPhone 14 series
    'iPhone14,7': 'iPhone 14',
    'iPhone14,8': 'iPhone 14 Plus',
    'iPhone15,2': 'iPhone 14 Pro',
    'iPhone15,3': 'iPhone 14 Pro Max',
    // iPhone 13 & SE (3rd gen)
    'iPhone14,2': 'iPhone 13 Pro',
    'iPhone14,3': 'iPhone 13 Pro Max',
    'iPhone14,4': 'iPhone 13 mini',
    'iPhone14,5': 'iPhone 13',
    'iPhone14,6': 'iPhone SE (3rd gen)',
    // iPhone 12 series
    'iPhone13,1': 'iPhone 12 mini',
    'iPhone13,2': 'iPhone 12',
    'iPhone13,3': 'iPhone 12 Pro',
    'iPhone13,4': 'iPhone 12 Pro Max',
    // iPhone 11 & SE (2nd gen)
    'iPhone12,1': 'iPhone 11',
    'iPhone12,3': 'iPhone 11 Pro',
    'iPhone12,5': 'iPhone 11 Pro Max',
    'iPhone12,8': 'iPhone SE (2nd gen)',
    // iPhone X / XS / XR / 8
    'iPhone11,2': 'iPhone XS',
    'iPhone11,4': 'iPhone XS Max',
    'iPhone11,6': 'iPhone XS Max',
    'iPhone11,8': 'iPhone XR',
    'iPhone10,3': 'iPhone X',
    'iPhone10,6': 'iPhone X',
    'iPhone10,1': 'iPhone 8',
    'iPhone10,4': 'iPhone 8',
    'iPhone10,2': 'iPhone 8 Plus',
    'iPhone10,5': 'iPhone 8 Plus',
    // Recent iPads
    'iPad16,3': 'iPad Pro 11-inch (M4)',
    'iPad16,4': 'iPad Pro 11-inch (M4)',
    'iPad16,5': 'iPad Pro 13-inch (M4)',
    'iPad16,6': 'iPad Pro 13-inch (M4)',
    'iPad16,1': 'iPad mini (A17 Pro)',
    'iPad16,2': 'iPad mini (A17 Pro)',
    'iPad14,8': 'iPad Air 11-inch (M2)',
    'iPad14,9': 'iPad Air 11-inch (M2)',
    'iPad14,10': 'iPad Air 13-inch (M2)',
    'iPad14,11': 'iPad Air 13-inch (M2)',
    'iPad14,3': 'iPad Pro 11-inch (4th gen)',
    'iPad14,4': 'iPad Pro 11-inch (4th gen)',
    'iPad14,5': 'iPad Pro 12.9-inch (6th gen)',
    'iPad14,6': 'iPad Pro 12.9-inch (6th gen)',
    'iPad14,1': 'iPad mini (6th gen)',
    'iPad14,2': 'iPad mini (6th gen)',
    'iPad13,18': 'iPad (10th gen)',
    'iPad13,19': 'iPad (10th gen)',
    'iPad13,16': 'iPad Air (5th gen)',
    'iPad13,17': 'iPad Air (5th gen)',
    'iPad13,4': 'iPad Pro 11-inch (3rd gen)',
    'iPad13,5': 'iPad Pro 11-inch (3rd gen)',
    'iPad13,6': 'iPad Pro 11-inch (3rd gen)',
    'iPad13,7': 'iPad Pro 11-inch (3rd gen)',
    'iPad13,8': 'iPad Pro 12.9-inch (5th gen)',
    'iPad13,9': 'iPad Pro 12.9-inch (5th gen)',
    'iPad13,10': 'iPad Pro 12.9-inch (5th gen)',
    'iPad13,11': 'iPad Pro 12.9-inch (5th gen)',
    'iPad13,1': 'iPad Air (4th gen)',
    'iPad13,2': 'iPad Air (4th gen)',
  };

  final mapped = table[id];
  if (mapped != null) return mapped;

  // Graceful fallback for newer/unlisted models (never return raw "x86_64" or "arm64").
  if (id.startsWith('iPhone')) return 'iPhone';
  if (id.startsWith('iPad')) return 'iPad';
  if (id.startsWith('iPod')) return 'iPod touch';
  return '';
}
