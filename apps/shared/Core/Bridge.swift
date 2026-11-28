import Foundation
import TelevisorFFI

/// The Swift side of `crates/ffi`: one JSON request in, one JSON response
/// out. The wire format is documented in `crates/ffi/src/protocol.rs`.
///
/// `televisor_call` blocks until the panel answers, so it runs on a GCD
/// queue, never on Swift's cooperative pool, whose few threads must not
/// block.
enum Bridge {
    enum Method: String, Sendable {
        case open, close, status, refresh, categories, browse, search
        case movie, series, epg, play, history, favorites
        case epgNow = "epg_now"
        case setFavorite = "set_favorite"
        case sportsToday = "sports_today"
        case remuxOpen = "remux_open"
        case remuxClose = "remux_close"
    }

    struct NoParams: Encodable, Sendable {}

    /// Stands in for responses with no payload (`{"ok": null}`).
    struct Empty: Decodable, Sendable {}

    static func call<Response: Decodable & Sendable>(
        _ method: Method,
        _ params: some Encodable & Sendable = NoParams()
    ) async throws -> Response {
        let request = try encoder.encode(Request(method: method.rawValue, params: params))
        precondition(request.count < requestBytesMax, "requests are small by construction")
        let response = await send(request)
        let reply: Reply<Response>
        do {
            reply = try JSONDecoder.bridge.decode(Reply<Response>.self, from: response)
        } catch {
            // The two sides disagree on a shape: a bug, not a user error.
            assertionFailure("undecodable \(method.rawValue) response: \(error)")
            throw BridgeError(kind: .server, message: "unreadable response: \(error)")
        }
        if let failure = reply.error {
            throw failure
        }
        if let value = reply.ok {
            return value
        }
        if let empty = Empty() as? Response {
            return empty
        }
        throw BridgeError(kind: .server, message: "\(method.rawValue) returned nothing")
    }

    /// Mirrors `REQUEST_BYTES_MAX` in `crates/ffi/src/protocol.rs`.
    private static let requestBytesMax = 64 * 1024

    private static let queue = DispatchQueue(
        label: "sh.televisor.bridge",
        qos: .userInitiated,
        attributes: .concurrent
    )

    private static var encoder: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        return encoder
    }

    private static func send(_ request: Data) async -> Data {
        await withCheckedContinuation { continuation in
            queue.async {
                // JSON never holds a NUL byte, so this is the whole request.
                let text = String(decoding: request, as: UTF8.self)
                let response = text.withCString { pointer -> Data in
                    guard let raw = televisor_call(pointer) else {
                        preconditionFailure("televisor_call never returns NULL")
                    }
                    defer { televisor_string_free(raw) }
                    return Data(bytes: raw, count: strlen(raw))
                }
                continuation.resume(returning: response)
            }
        }
    }

    private struct Request<Params: Encodable>: Encodable {
        let method: String
        let params: Params

        private enum CodingKeys: String, CodingKey { case method, params }

        /// Methods without parameters are unit variants in Rust, which
        /// accept no `params` key at all, not an empty object.
        func encode(to encoder: any Encoder) throws {
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encode(method, forKey: .method)
            if !(params is NoParams) {
                try container.encode(params, forKey: .params)
            }
        }
    }

    private struct Reply<Value: Decodable>: Decodable {
        let ok: Value?
        let error: BridgeError?
    }
}

extension JSONDecoder {
    static var bridge: JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }
}

/// An error the bridge reports. `kind` is the closed set from
/// `ErrorKind` in `crates/ffi/src/protocol.rs`.
struct BridgeError: Error, Decodable, Sendable, Equatable {
    enum Kind: String, Decodable, Sendable {
        case invalidRequest = "invalid_request"
        case notSignedIn = "not_signed_in"
        case authFailed = "auth_failed"
        case invalidHost = "invalid_host"
        case network
        case server
        case paymentRequired = "payment_required"
        case liveOnly = "live_only"
        case notFound = "not_found"
        case storage
        case unsupported
    }

    let kind: Kind
    /// English detail from Rust; secondary text only.
    let message: String

    var title: String {
        switch kind {
        case .authFailed: "Sign-in Failed"
        case .invalidHost: "Check the Server Address"
        case .network: "Can’t Reach Your Provider"
        case .server: message.hasSuffix(".") ? "Can’t Play This" : "Your Provider Had a Problem"
        case .paymentRequired: "Payment Required"
        case .liveOnly: "Live TV Only"
        case .notFound: "No Longer Available"
        case .storage: "Storage Problem"
        case .notSignedIn: "Signed Out"
        case .invalidRequest: "Something Went Wrong"
        case .unsupported: "Can’t Play This"
        }
    }

    var explanation: String {
        switch kind {
        case .authFailed: "The username or password was not accepted."
        case .invalidHost: "Enter an address like panel.example.com:8080."
        case .network: "Check your internet connection and try again."
        case .server: message.hasSuffix(".") ? message : "The service didn’t answer properly. Try again in a moment."
        case .paymentRequired: "This provider requires a payment before it will stream."
        case .liveOnly: "This account only includes live channels."
        case .notFound: "This title was removed by your provider."
        case .storage: "Apple TV couldn’t save data. Free up space and try again."
        case .notSignedIn: "Sign in again to continue."
        case .invalidRequest: message
        case .unsupported: message
        }
    }
}
