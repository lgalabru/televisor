import Foundation
import Security

/// The signed-in account, in the Keychain: it holds a password, and the
/// Keychain is the one tvOS store that survives cache purges.
enum CredentialStore {
    private static let service = "sh.televisor.tv.account"
    private static let account = "default"

    static func load() -> Televisor.Credentials? {
        let query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
            kSecReturnData: true,
            kSecMatchLimit: kSecMatchLimitOne,
        ]
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess, let data = result as? Data else {
            // errSecItemNotFound is the normal first launch; anything else
            // means the item is unusable, and signing in again replaces it.
            return nil
        }
        return try? JSONDecoder().decode(Televisor.Credentials.self, from: data)
    }

    static func save(_ credentials: Televisor.Credentials) {
        guard let data = try? JSONEncoder().encode(credentials) else {
            preconditionFailure("credentials always encode")
        }
        clear()
        let item: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
            kSecAttrAccessible: kSecAttrAccessibleAfterFirstUnlock,
            kSecValueData: data,
        ]
        let status = SecItemAdd(item as CFDictionary, nil)
        // Not fatal: the session is open; the app asks again next launch.
        if status != errSecSuccess {
            print("televisor: could not save the account to the Keychain (\(status))")
        }
    }

    static func clear() {
        let query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
        ]
        // errSecItemNotFound is fine: there was nothing to clear.
        _ = SecItemDelete(query as CFDictionary)
    }
}

/// The account `apps/appletv/scripts/embed-seed.sh` bakes into personal
/// builds, so the first launch needs no typing.
enum SeedAccount {
    static func load() -> Televisor.Credentials? {
        guard let url = Bundle.main.url(forResource: "seed-account", withExtension: "json"),
              let data = try? Data(contentsOf: url)
        else {
            return nil
        }
        return try? JSONDecoder.bridge.decode(Televisor.Credentials.self, from: data)
    }
}
