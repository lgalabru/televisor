import Foundation

/// Panels name categories "FR| SPORT HD/4K", "US| NFL PPV": the prefix is a
/// country or language. Grouping by it turns 400 categories into a dozen
/// regions, and lets the viewer's own come first.
struct Region: Hashable, Identifiable, Sendable {
    let code: String
    var id: String { code }

    var name: String {
        switch code {
        case "FR": "France"
        case "US": "United States"
        case "UK": "United Kingdom"
        case "CA": "Canada"
        case "ES": "Spain"
        case "PT": "Portugal"
        case "IE": "Ireland"
        case "DE": "Germany"
        case "IT": "Italy"
        case "BE": "Belgium"
        case "NL": "Netherlands"
        // In IPTV lists "AR" is the Arabic world, not Argentina.
        case "AR": "Arabic"
        case "LA": "Latin America"
        case "AFR": "Africa"
        case "4K", "8K": "Ultra HD"
        default: code
        }
    }

    var flag: String {
        switch code {
        case "UK": return "🇬🇧"
        case "AR": return "🌐"
        case "LA": return "🌎"
        case "AFR": return "🌍"
        default:
            guard code.count == 2, code.allSatisfy({ $0.isASCII && $0.isLetter }) else { return "📺" }
            // Regional indicator symbols spell the flag of a country code.
            let scalars = code.uppercased().unicodeScalars.compactMap {
                UnicodeScalar(0x1F1E6 - 0x41 + $0.value)
            }
            return String(String.UnicodeScalarView(scalars))
        }
    }

    /// "FR" for "FR| SPORT HD/4K"; nil for a category without a prefix.
    static func code(ofCategory name: String) -> String? {
        guard let match = name.firstMatch(of: /^\s*([A-Z0-9]{2,3})\s*\|/) else { return nil }
        return String(match.output.1)
    }

    /// "FR" for "Live | … | FR: SOCCER PPV 28" or "FR: TF1 HD".
    static func code(ofChannel name: String) -> String? {
        if let match = name.firstMatch(of: /(?:^|\|\s*)([A-Z]{2,3}):\s/) {
            return String(match.output.1)
        }
        return nil
    }
}

/// Live categories grouped by region, the preferred region first.
struct RegionGroups {
    struct Group: Identifiable {
        let region: Region
        let categories: [Category]
        var id: String { region.id }
    }

    let groups: [Group]

    init(_ categories: [Category], preferred: String) {
        var byCode: [String: [Category]] = [:]
        var order: [String] = []
        for category in categories {
            // "CA| SPORTS FR": French-language categories of other regions
            // belong with the language.
            let suffix = category.name.split(separator: " ").last.map(String.init)
            let code = suffix == preferred ? preferred : (Region.code(ofCategory: category.name) ?? "Other")
            if byCode[code] == nil {
                order.append(code)
            }
            byCode[code, default: []].append(category)
        }
        order.sort { left, right in
            if left == preferred { return true }
            if right == preferred { return false }
            return (byCode[left]?.count ?? 0) > (byCode[right]?.count ?? 0)
        }
        groups = order.map { Group(region: Region(code: $0), categories: byCode[$0] ?? []) }
    }

    /// The categories shown under a region heading, without the prefix.
    static func shortName(_ category: Category) -> String {
        category.name.replacing(/^\s*[A-Z0-9]{2,3}\s*\|\s*/, with: "")
    }
}
