/**
 * Helper to make all paths safe, can specify if should start or end with a slash.
 * Works with filepaths and urls, with dynamic defaults depending on what comes in.
 */
export const genPath = (
    path_in: string,
    {
        sShlash = undefined, // If isn't a root url (e.g. starting with http), or a relative path, then will default to true, otherwise false
        eSlash = undefined, // NOTE! defaults to true if dir, false if file
        extra = undefined, // Extra sections to add to the end of the path, the sSlash/eSlash applies after this has been added:
    }: {
        sShlash?: boolean;
        eSlash?: boolean;
        extra?: string[];
    } = {},
): string => {
    // Strip any leading or trailing whitespace:
    let path = path_in.trim();
    if (extra) {
        extra = extra.map((e) => e.trim());
    }

    if (extra) {
        // Adding sections, start with / at end:
        if (!path.endsWith("/")) {
            path = `${path}/`;
        }

        extra.forEach((e) => {
            // Add each section, existing path ends with /:
            path = `${path}${e}/`;
        });
    }

    // Decide whether sSlash should default to true or false depending on whether it looks like a root url:
    if (sShlash === undefined) {
        sShlash = !path.startsWith("http") && !path.startsWith(".");
    }

    // Decide whether eSlash should default to true or false
    if (eSlash === undefined) {
        // Decide if the path seems to be a file or a dir.
        // Do this by checking if the final section contains a dot:
        // Making sure to ignore a slash at the end of the string:
        const tmpPath = path.endsWith("/") ? path.slice(0, -1) : path;
        const isFile = tmpPath.split("/").pop()?.includes(".");
        eSlash = !isFile;
    }

    // Apply the final slash rules:
    if (sShlash && !path.startsWith("/")) {
        path = `/${path}`;
    } else if (!sShlash && path.startsWith("/")) {
        path = path.slice(1);
    }

    if (eSlash && !path.endsWith("/")) {
        path = `${path}/`;
    } else if (!eSlash && path.endsWith("/")) {
        path = path.slice(0, -1);
    }

    return path;
};
