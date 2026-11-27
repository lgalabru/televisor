import SwiftUI

/// Shown only when no saved or seeded account works. Typing on a Siri
/// Remote is slow, so the form accepts the playlist link providers email
/// out: its username and password fill themselves in. A nearby iPhone
/// offers its keyboard for every field.
struct SignInView: View {
    @Environment(AppModel.self) private var model
    @State private var host = ""
    @State private var username = ""
    @State private var password = ""
    @State private var isSigningIn = false
    @State private var failure: BridgeError?
    @FocusState private var focus: Field?

    private enum Field { case host, username, password, submit }

    var body: some View {
        #if os(tvOS)
        HStack(alignment: .center, spacing: 120) {
            intro.frame(maxWidth: 700, alignment: .leading)
            form.frame(width: 760)
        }
        .padding(80)
        .onAppear { focus = .host }
        #else
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: 40) {
                intro
                form
            }
            .padding(28)
            .frame(maxWidth: 560)
            .frame(maxWidth: .infinity)
        }
        .scrollDismissesKeyboard(.interactively)
        #endif
    }

    private var intro: some View {
            VStack(alignment: .leading, spacing: 28) {
                Wordmark()
                Text("Your IPTV subscription, on the big screen.")
                    .font(.title3)
                    .foregroundStyle(.secondary)
                Text("Enter the server your provider gave you. A full playlist link works too: the username and password are read from it.")
                    .font(.callout)
                    .foregroundStyle(.tertiary)
                    .frame(maxWidth: 640, alignment: .leading)
            }
    }

    private var form: some View {
            VStack(spacing: Theme.itemSpacing * 0.7) {
                TextField("Server, like panel.example.com:8080", text: $host)
                    .textContentType(.URL)
                    .focused($focus, equals: .host)
                    .onSubmit { focus = .username }
                TextField("Username", text: $username)
                    .textContentType(.username)
                    .focused($focus, equals: .username)
                    .onSubmit { focus = .password }
                SecureField("Password", text: $password)
                    .textContentType(.password)
                    .focused($focus, equals: .password)
                    .onSubmit { focus = .submit }
                if let failure {
                    Label(failure.explanation, systemImage: "exclamationmark.triangle.fill")
                        .font(.callout)
                        .foregroundStyle(Theme.accent)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                Button(action: submit) {
                    HStack(spacing: 16) {
                        if isSigningIn {
                            ProgressView()
                        }
                        Text(isSigningIn ? "Signing In…" : "Sign In")
                    }
                    .frame(maxWidth: .infinity)
                }
                .focused($focus, equals: .submit)
                .disabled(host.trimmingCharacters(in: .whitespaces).isEmpty || isSigningIn)
                .buttonStyle(.glassProminent)
            }
            #if os(iOS)
            .textFieldStyle(.roundedBorder)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            #endif
    }

    private func submit() {
        isSigningIn = true
        failure = nil
        let credentials = Televisor.Credentials(
            accountName: "default", host: host, username: username, password: password
        )
        Task {
            defer { isSigningIn = false }
            do {
                try await model.signIn(credentials)
            } catch let error as BridgeError {
                failure = error
            } catch {
                failure = BridgeError(kind: .storage, message: error.localizedDescription)
            }
        }
    }
}
