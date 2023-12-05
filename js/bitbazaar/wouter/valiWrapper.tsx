import React, { useMemo } from "react";

export const ValiWrapper = ({
    params,
    validators,
    Comp,
    Comp404,
}: {
    params: object;
    validators: Record<string, (param: string) => unknown>;
    Comp: React.ComponentType;
    Comp404: React.ComponentType;
}) => {
    const cleanedParams: object | null = useMemo(() => {
        try {
            const cleaned = {};
            Object.entries(params).forEach(([key, value]) => {
                cleaned[key] = validators[key](value as string);
            });
            return cleaned;
        } catch (e) {
            return null;
        }
    }, [validators, params]);

    if (cleanedParams === null) {
        return <Comp404 />;
    }

    return <Comp {...cleanedParams} />;
};
