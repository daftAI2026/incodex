#!/usr/bin/env swift
import AVFoundation
import CoreMedia
import CoreVideo
import Foundation
import Vision

private struct Options {
    var video: String?
    var label = "capture"
    var seedTime: Double?
    var seedAnchorX: Double?
    var seedAnchorY: Double?
    var seedAnchorWidth: Double?
    var seedAnchorHeight: Double?
    var duration = 1.0
    var gapMultiplier = 1.5
    var selfTest = false
    var help = false
}

private struct Gap: Codable {
    let fromPresentationTimeSec: Double
    let toPresentationTimeSec: Double
    let intervalMs: Double
    let medianIntervalsSpanned: Double
}

private struct Observation: Codable {
    let presentationTimeSec: Double
    let deltaFromPreviousMs: Double?
    let anchorXPx: Double
    let anchorYPx: Double
    let anchorWidthPx: Double
    let anchorHeightPx: Double
    let anchorCentroidXPx: Double
    let anchorCentroidYPx: Double
    let confidence: Double?
    let source: String
}

private struct Report: Codable {
    let label: String
    let video: String
    let widthPx: Int
    let heightPx: Int
    let requestedSeedTimeSec: Double
    let actualSeedFrameTimeSec: Double
    let measuredThroughPresentationTimeSec: Double
    let medianFrameIntervalMs: Double
    let gapThresholdMs: Double
    let observations: [Observation]
    let possibleVFRGaps: [Gap]
    let confidenceMeaning: String
    let trackedFeature: String
    let limitations: [String]
}

private enum AnalyzerError: Error, CustomStringConvertible {
    case usage(String)
    case media(String)
    case tracking(String)

    var description: String {
        switch self {
        case .usage(let message), .media(let message), .tracking(let message): return message
        }
    }
}

private func usage() -> String {
    """
    Offline Accessibility icon-anchor tracker. Pixel coordinates use the encoded frame's top-left origin.

    swift scripts/flight-video-measure.swift \\
      --video /path/capture.mp4 --label original --seed-time 55.0 \\
      --seed-anchor-x 893 --seed-anchor-y 338 --seed-anchor-width 56 --seed-anchor-height 56 --duration 0.8

    Required: --video, --seed-time, --seed-anchor-x, --seed-anchor-y,
              --seed-anchor-width, --seed-anchor-height
    Optional: --label TEXT, --duration SECONDS (default 1.0),
              --gap-multiplier N (default 1.5), --help, --self-test

    Supply a seed rectangle around the visible Accessibility glyph/icon. Every reported
    geometry field describes that tracked anchor only; this tool does not infer card bounds.
    JSON is written to stdout. Input media is read only; no frames or sidecars are written.
    """
}

private func parseOptions(_ args: [String]) throws -> Options {
    var result = Options()
    var index = 0
    while index < args.count {
        let flag = args[index]
        if flag == "--self-test" { result.selfTest = true; index += 1; continue }
        if flag == "--help" || flag == "-h" { result.help = true; index += 1; continue }
        guard index + 1 < args.count, !args[index + 1].hasPrefix("--") else {
            throw AnalyzerError.usage("missing value for \(flag)\n\n\(usage())")
        }
        let value = args[index + 1]
        switch flag {
        case "--video": result.video = value
        case "--label": result.label = value
        case "--seed-time": result.seedTime = try number(value, name: flag)
        case "--seed-anchor-x": result.seedAnchorX = try number(value, name: flag)
        case "--seed-anchor-y": result.seedAnchorY = try number(value, name: flag)
        case "--seed-anchor-width": result.seedAnchorWidth = try number(value, name: flag)
        case "--seed-anchor-height": result.seedAnchorHeight = try number(value, name: flag)
        case "--duration": result.duration = try number(value, name: flag)
        case "--gap-multiplier": result.gapMultiplier = try number(value, name: flag)
        default: throw AnalyzerError.usage("unknown option: \(flag)\n\n\(usage())")
        }
        index += 2
    }

    if result.help || result.selfTest {
        guard args.count == 1 else { throw AnalyzerError.usage("--help and --self-test must be used alone") }
        return result
    }
    guard let video = result.video, !video.isEmpty,
          let seedTime = result.seedTime,
          result.seedAnchorX != nil, result.seedAnchorY != nil,
          result.seedAnchorWidth != nil, result.seedAnchorHeight != nil else {
        throw AnalyzerError.usage("--video, --seed-time, and the four --seed-anchor-* pixel values are required\n\n\(usage())")
    }
    guard seedTime >= 0, result.duration > 0, result.duration <= 10,
          result.gapMultiplier > 1, result.gapMultiplier <= 10,
          (result.seedAnchorX ?? -1) >= 0, (result.seedAnchorY ?? -1) >= 0,
          (result.seedAnchorWidth ?? 0) > 0, (result.seedAnchorHeight ?? 0) > 0,
          !result.label.isEmpty else {
        throw AnalyzerError.usage("seed-anchor geometry/time, duration, gap multiplier, or label is outside its allowed range")
    }
    return result
}

private func number(_ value: String, name: String) throws -> Double {
    guard let parsed = Double(value), parsed.isFinite else {
        throw AnalyzerError.usage("\(name) must be a finite number: \(value)")
    }
    return parsed
}

private func median(_ values: [Double]) -> Double {
    guard !values.isEmpty else { return 0 }
    let ordered = values.sorted()
    let middle = ordered.count / 2
    if ordered.count.isMultiple(of: 2) {
        return (ordered[middle - 1] + ordered[middle]) / 2
    }
    return ordered[middle]
}

private func validConfidence(_ value: Double) -> Bool {
    value.isFinite && (0.0...1.0).contains(value)
}

private func possibleGaps(timestamps: [Double], multiplier: Double) -> (medianInterval: Double, gaps: [Gap]) {
    let intervals = zip(timestamps.dropFirst(), timestamps).map { max(0, $0.0 - $0.1) }.filter { $0 > 0 }
    let middle = median(intervals)
    guard middle > 0 else { return (middle, []) }
    let threshold = middle * multiplier
    let gaps = zip(timestamps.dropFirst(), timestamps).compactMap { current, previous -> Gap? in
        let delta = current - previous
        guard delta > threshold else { return nil }
        return Gap(fromPresentationTimeSec: previous,
                   toPresentationTimeSec: current,
                   intervalMs: delta * 1_000,
                   medianIntervalsSpanned: delta / middle)
    }
    return (middle, gaps)
}

private func topLeftPixelRect(_ rect: CGRect, width: Int, height: Int) -> (x: Double, y: Double, width: Double, height: Double, centerX: Double, centerY: Double) {
    let x = Double(rect.minX) * Double(width)
    let y = (1 - Double(rect.maxY)) * Double(height)
    let w = Double(rect.width) * Double(width)
    let h = Double(rect.height) * Double(height)
    return (x, y, w, h, x + w / 2, y + h / 2)
}

private func makeObservation(time: Double,
                             deltaMs: Double?,
                             rect: (x: Double, y: Double, width: Double, height: Double, centerX: Double, centerY: Double),
                             confidence: Double?,
                             source: String) -> Observation {
    Observation(presentationTimeSec: time,
                deltaFromPreviousMs: deltaMs,
                anchorXPx: rect.x,
                anchorYPx: rect.y,
                anchorWidthPx: rect.width,
                anchorHeightPx: rect.height,
                anchorCentroidXPx: rect.centerX,
                anchorCentroidYPx: rect.centerY,
                confidence: confidence,
                source: source)
}

private func runSelfTest() throws {
    let converted = topLeftPixelRect(CGRect(x: 0.25, y: 0.5, width: 0.25, height: 0.25), width: 1_920, height: 1_080)
    guard converted.x == 480, converted.y == 270,
          converted.width == 480, converted.height == 270,
          converted.centerX == 720, converted.centerY == 405 else {
        throw AnalyzerError.tracking("self-test failed: normalized Vision bounds did not convert to top-left pixels")
    }
    let gaps = possibleGaps(timestamps: [0, 0.016, 0.032, 0.064, 0.080], multiplier: 1.5)
    guard abs(gaps.medianInterval - 0.016) < 0.000_001,
          gaps.gaps.count == 1,
          abs(gaps.gaps[0].intervalMs - 32) < 0.001,
          abs(gaps.gaps[0].medianIntervalsSpanned - 2) < 0.001 else {
        throw AnalyzerError.tracking("self-test failed: long VFR interval was not accounted for")
    }
    guard validConfidence(0.83), !validConfidence(-0.01), !validConfidence(1.01) else {
        throw AnalyzerError.tracking("self-test failed: confidence range check")
    }
    let observation = Observation(presentationTimeSec: 1, deltaFromPreviousMs: nil,
                                  anchorXPx: 4, anchorYPx: 5,
                                  anchorWidthPx: 56, anchorHeightPx: 56,
                                  anchorCentroidXPx: 32, anchorCentroidYPx: 33,
                                  confidence: 0.8, source: "self-test")
    let schema = try JSONSerialization.jsonObject(with: JSONEncoder().encode(observation)) as? [String: Any]
    guard schema?["anchorCentroidXPx"] != nil, schema?["anchorWidthPx"] != nil,
          schema?["centroidXPx"] == nil, schema?["widthPx"] == nil else {
        throw AnalyzerError.tracking("self-test failed: observation schema is not anchor-only")
    }
    print("self-test passed; anchorCentroidXPx anchorWidthPx")
}

private func run(_ options: Options) throws {
    guard let videoPath = options.video,
          let seedTime = options.seedTime,
          let seedX = options.seedAnchorX, let seedY = options.seedAnchorY,
          let seedWidth = options.seedAnchorWidth, let seedHeight = options.seedAnchorHeight else {
        throw AnalyzerError.usage("incomplete measurement options")
    }
    let url = URL(fileURLWithPath: videoPath).standardizedFileURL
    guard FileManager.default.fileExists(atPath: url.path) else {
        throw AnalyzerError.media("video does not exist: \(url.path)")
    }

    let asset = AVURLAsset(url: url)
    guard let track = asset.tracks(withMediaType: .video).first else {
        throw AnalyzerError.media("video has no video track: \(url.path)")
    }
    let reader = try AVAssetReader(asset: asset)
    let settings: [String: Any] = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
    let output = AVAssetReaderTrackOutput(track: track, outputSettings: settings)
    output.alwaysCopiesSampleData = false
    guard reader.canAdd(output) else { throw AnalyzerError.media("AVAssetReader rejected BGRA video output") }
    reader.add(output)
    let rangeStart = CMTime(seconds: seedTime, preferredTimescale: 600)
    let rangeDuration = CMTime(seconds: options.duration, preferredTimescale: 600)
    reader.timeRange = CMTimeRange(start: rangeStart, duration: rangeDuration)
    guard reader.startReading() else {
        throw AnalyzerError.media("could not start video reader: \(reader.error?.localizedDescription ?? "unknown error")")
    }

    let sequence = VNSequenceRequestHandler()
    var tracker: VNTrackObjectRequest?
    var frameWidth = 0
    var frameHeight = 0
    var seedFrameTime: Double?
    var observations: [Observation] = []
    var timestamps: [Double] = []
    let endTime = seedTime + options.duration

    while reader.status == .reading, let sample = output.copyNextSampleBuffer() {
        let time = CMSampleBufferGetPresentationTimeStamp(sample).seconds
        guard time.isFinite else { continue }
        if time < seedTime { continue }
        if time > endTime { break }
        guard let pixelBuffer = CMSampleBufferGetImageBuffer(sample) else { continue }

        let width = CVPixelBufferGetWidth(pixelBuffer)
        let height = CVPixelBufferGetHeight(pixelBuffer)
        if tracker == nil {
            guard seedX + seedWidth <= Double(width), seedY + seedHeight <= Double(height) else {
                reader.cancelReading()
                throw AnalyzerError.usage("seed rectangle is outside the \(width)x\(height) video frame")
            }
            frameWidth = width
            frameHeight = height
            seedFrameTime = time
            let visionRect = CGRect(x: seedX / Double(width),
                                    y: 1 - (seedY + seedHeight) / Double(height),
                                    width: seedWidth / Double(width),
                                    height: seedHeight / Double(height))
            let seedObservation = VNDetectedObjectObservation(boundingBox: visionRect)
            let request = VNTrackObjectRequest(detectedObjectObservation: seedObservation)
            request.trackingLevel = .accurate
            tracker = request
            observations.append(makeObservation(time: time,
                                                deltaMs: nil,
                                                rect: (seedX, seedY, seedWidth, seedHeight, seedX + seedWidth / 2, seedY + seedHeight / 2),
                                                confidence: nil,
                                                source: "caller-seed"))
            timestamps.append(time)
            continue
        }

        guard let request = tracker else { continue }
        do {
            try sequence.perform([request], on: pixelBuffer)
        } catch {
            reader.cancelReading()
            throw AnalyzerError.tracking("Vision tracking failed at \(time)s: \(error.localizedDescription)")
        }
        guard let observation = request.results?.first as? VNDetectedObjectObservation else {
            reader.cancelReading()
            throw AnalyzerError.tracking("Vision returned no tracked observation at \(time)s")
        }
        let trackedAnchor = topLeftPixelRect(observation.boundingBox, width: frameWidth, height: frameHeight)
        guard validConfidence(Double(observation.confidence)) else {
            reader.cancelReading()
            throw AnalyzerError.tracking("Vision returned invalid confidence at \(time)s")
        }
        let previous = timestamps.last
        observations.append(makeObservation(time: time,
                                            deltaMs: previous.map { (time - $0) * 1_000 },
                                            rect: trackedAnchor,
                                            confidence: Double(observation.confidence),
                                            source: "vision-tracker"))
        timestamps.append(time)
        request.inputObservation = observation
        if observation.confidence < 0.1 { request.isLastFrame = true }
    }

    guard let actualSeedFrameTime = seedFrameTime, observations.count >= 2 else {
        throw AnalyzerError.media("no trackable frames at/after seed time \(seedTime)s")
    }
    if reader.status == .failed {
        throw AnalyzerError.media("video decode failed: \(reader.error?.localizedDescription ?? "unknown error")")
    }
    let gapSummary = possibleGaps(timestamps: timestamps, multiplier: options.gapMultiplier)
    let report = Report(label: options.label,
                        video: url.path,
                        widthPx: frameWidth,
                        heightPx: frameHeight,
                        requestedSeedTimeSec: seedTime,
                        actualSeedFrameTimeSec: actualSeedFrameTime,
                        measuredThroughPresentationTimeSec: timestamps.last ?? actualSeedFrameTime,
                        medianFrameIntervalMs: gapSummary.medianInterval * 1_000,
                        gapThresholdMs: gapSummary.medianInterval * options.gapMultiplier * 1_000,
                        observations: observations,
                        possibleVFRGaps: gapSummary.gaps,
                        confidenceMeaning: "Vision VNTrackObjectRequest confidence for the supplied Accessibility glyph/icon anchor rectangle in [0,1]; the caller seed has null confidence and is not a Vision result.",
                        trackedFeature: "Accessibility glyph/icon anchor only; observation bounds are the Vision tracked anchor rectangle.",
                        limitations: [
                            "The caller-supplied seed rectangle must tightly enclose the visible Accessibility glyph/icon at the actual seed PTS.",
                            "Anchor trajectory does not measure or infer the enclosing flight card's centroid, width, or height.",
                            "Intervals above the local median threshold are possible missing capture slots, not proof that the encoder dropped a frame; VFR may intentionally omit unchanged frames.",
                            "The captures have different permission states and interaction paths, so anchor trajectories are descriptive and are not strict same-condition parity.",
                        ])
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
    let data = try encoder.encode(report)
    guard let json = String(data: data, encoding: .utf8) else { throw AnalyzerError.media("failed to encode report as UTF-8") }
    print(json)
}

do {
    let options = try parseOptions(Array(CommandLine.arguments.dropFirst()))
    if options.help { print(usage()) }
    else if options.selfTest { try runSelfTest() }
    else { try run(options) }
} catch {
    fputs("flight-video-measure: \(error)\n", stderr)
    exit(2)
}
