export declare function open(opts?: {
    directory?: boolean;
    multiple?: boolean;
    filters?: Array<{
        name: string;
        extensions: string[];
    }>;
}): Promise<string | string[] | null>;
