using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
using AnixOps.Net.Ipc;

namespace AnixOps.Net.Session;

/// <summary>
/// Where one user trusts the HTTPS decryption root. The core runs in a system service and must not trust a
/// root for the whole machine at one user's word, so the window keeps it in that user's own store.
/// </summary>
public interface IRootStore
{
    /// <summary>Whether the store trusts the root <paramref name="certificatePem"/>.</summary>
    bool Trusts(string certificatePem);

    /// <summary>Adds the root. May wait for the user to confirm; a refusal is a <see cref="RefusedException"/>.</summary>
    void Trust(string certificatePem);

    /// <summary>Takes the root out again. Nothing happens when it is not there.</summary>
    void Untrust(string certificatePem);
}

/// <summary>
/// The signed-in user's "Trusted Root Certification Authorities". Needs no administrator rights and reaches
/// no other user. Windows itself asks the user to confirm before a root is added or removed.
/// </summary>
public sealed class UserRootStore : IRootStore
{
    // What CertAddCertificateContextToStore and its like report when the user chose No.
    private const int Cancelled = unchecked((int)0x800704C7);

    private static X509Certificate2 Parse(string certificatePem)
    {
        try
        {
            return X509Certificate2.CreateFromPem(certificatePem);
        }
        catch (CryptographicException problem)
        {
            throw new RefusedException($"the root certificate could not be read: {problem.Message}");
        }
    }

    private static X509Store Open(OpenFlags flags)
    {
        var store = new X509Store(StoreName.Root, StoreLocation.CurrentUser);
        store.Open(flags);
        return store;
    }

    public bool Trusts(string certificatePem)
    {
        using var certificate = Parse(certificatePem);
        using var store = Open(OpenFlags.ReadOnly);
        return store.Certificates.Find(X509FindType.FindByThumbprint, certificate.Thumbprint, validOnly: false).Count > 0;
    }

    public void Trust(string certificatePem)
    {
        using var certificate = Parse(certificatePem);
        try
        {
            using var store = Open(OpenFlags.ReadWrite);
            store.Add(certificate);
        }
        catch (CryptographicException problem)
        {
            throw new RefusedException(problem.HResult == Cancelled
                ? "the root certificate was not trusted: the Windows security warning was answered with No"
                : $"the root certificate could not be added to your certificate store: {problem.Message}");
        }
    }

    public void Untrust(string certificatePem)
    {
        using var certificate = Parse(certificatePem);
        try
        {
            using var store = Open(OpenFlags.ReadWrite);
            foreach (var found in store.Certificates.Find(X509FindType.FindByThumbprint, certificate.Thumbprint, validOnly: false))
            {
                store.Remove(found);
                found.Dispose();
            }
        }
        catch (CryptographicException problem)
        {
            throw new RefusedException(problem.HResult == Cancelled
                ? "the root certificate is still trusted: the Windows confirmation was answered with No"
                : $"the root certificate could not be removed from your certificate store: {problem.Message}");
        }
    }
}
