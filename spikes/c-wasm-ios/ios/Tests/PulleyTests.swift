import XCTest

final class PulleyTests: XCTestCase {
    /// Runs the Plugin component under Pulley and the native baseline.
    /// The report is printed with a fixed marker so it can be found in the
    /// Test Lab xcodebuild log, and attached to the test result.
    func testPulleyComponentRuns() throws {
        guard let raw = ristle_spike_run() else { return XCTFail("no report") }
        let report = String(cString: raw)
        ristle_spike_free(raw)
        print("RISTLE_SPIKE_RESULT \(report)")
        NSLog("RISTLE_SPIKE_RESULT %@", report)
        let attachment = XCTAttachment(string: report)
        attachment.name = "ristle-spike-report.json"
        attachment.lifetime = .keepAlways
        add(attachment)

        let data = try XCTUnwrap(report.data(using: .utf8))
        let modes = try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [[String: Any]])
        let pulley = try XCTUnwrap(modes.first { $0["mode"] as? String == "pulley" })
        let native = try XCTUnwrap(modes.first { $0["mode"] as? String == "native" })
        XCTAssertNil(pulley["error"], "Pulley failed: \(pulley["error"] ?? "")")
        XCTAssertEqual(pulley["search_hits"] as? Int, native["search_hits"] as? Int)
        XCTAssertEqual(pulley["decrypt_checksum"] as? UInt64, native["decrypt_checksum"] as? UInt64)
    }
}
