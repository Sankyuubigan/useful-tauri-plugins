export interface YtdlpStatus {
    installed: boolean;
    version?: string | null;
    exe_path: string;
    dir: string;
    ffmpeg_installed: boolean;
    ffmpeg_path?: string | null;
    deno_installed: boolean;
    deno_path?: string | null;
    message: string;
}
export interface VideoInfo {
    title?: string;
    duration?: number;
    thumbnail?: string;
    uploader?: string;
    description?: string;
    [key: string]: unknown;
}
export declare function getYtdlpStatus(): Promise<YtdlpStatus>;
export declare function installYtdlp(): Promise<YtdlpStatus>;
export declare function checkYtdlpUpdate(): Promise<string | null>;
export declare function installYtdlpUpdate(): Promise<YtdlpStatus>;
export declare function setYtdlpDir(path: string): Promise<YtdlpStatus>;
export declare function setFfmpegPath(path: string): Promise<YtdlpStatus>;
export declare function installDeno(): Promise<YtdlpStatus>;
export declare function fetchVideoInfo(url: string): Promise<VideoInfo>;
export interface DownloadParams {
    url: string;
    format: string;
    quality: string;
    outputDir: string;
    authMode: string;
    browser?: string | null;
    cookiesFile?: string | null;
}
export declare function downloadVideo(params: DownloadParams): Promise<void>;
export declare function cancelDownload(): Promise<void>;
import './web-components';
