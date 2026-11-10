export interface FfmpegStatus {
    installed: boolean;
    ffmpegVersion: string | null;
    ffprobeVersion: string | null;
    dir: string;
    binDir: string;
    message: string;
}
export declare function getFfmpegStatus(dir: string): Promise<FfmpegStatus>;
