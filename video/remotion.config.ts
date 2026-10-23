import { Config } from "@remotion/cli/config";

// A near-lossless master; scripts/render.sh encodes the README files from it.
Config.setVideoImageFormat("jpeg");
Config.setJpegQuality(95);
Config.setCodec("h264");
Config.setCrf(14);
Config.setPixelFormat("yuv420p");
Config.setConcurrency(2);
